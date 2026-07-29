import json

# ── AST helpers ──────────────────────────────────────────────────────────────
def ref(i): return {"op":"ref","element_id":i}
def lit(v): return {"op":"literal","value":v}
IR = {"op":"index_ref","axis":"row"}
def idx(arr): return {"op":"index","array":ref(arr),"indices":[IR]}
def b(op,l,r): return {"op":op,"left":l,"right":r}
def call(fn,*a): return {"op":"call","fn":fn,"args":list(a)}
def vmap(body): return {"op":"vector_map","over":"Fleet","body":body}
def iff(c,t,e): return {"op":"if","cond":c,"then":t,"else":e}

def refs_of(ast, acc):
    if isinstance(ast,dict):
        if ast.get("op")=="ref": acc.add(ast["element_id"])
        for v in ast.values(): refs_of(v,acc)
    elif isinstance(ast,list):
        for v in ast: refs_of(v,acc)

def node(nid, name, ast, dims=False, save=None, extra_inputs=()):
    acc=set(); refs_of(ast,acc)
    acc.update(extra_inputs)
    e={"id":nid,"name":name,"primitive":"node","value_rule":"expression",
       "inputs":sorted(acc),"expression":{"ast":ast}}
    if dims: e["outputs"]=[{"name":name,"unit":"1","dimensions":["Fleet"]}]
    if save: e["save_results"]=save
    return e

def lagnode(nid,name,inp,dims=True):
    e={"id":nid,"name":name,"primitive":"node","value_rule":"lag",
       "input":inp,"initial":{"value":0,"unit":"1"}}
    if dims: e["outputs"]=[{"name":name,"unit":"1","dimensions":["Fleet"]}]
    return e

# ── Subsystems: (name, exponent, nominal per-step scale) ─────────────────────
SUBS = [("tires",4.0,0.020),("drive",3.0,0.011),("struct",1.5,0.005)]

els=[]
# shared inputs
els.append({"id":"dispatch_policy","name":"Dispatch policy","primitive":"node","value_rule":"fixed",
            "editable":True,"value":{"value":1,"unit":"1"}})
els.append({"id":"wear_mult","name":"Wear multiplier (uncertainty)","primitive":"node","value_rule":"sample",
            "distribution":{"family":"triangular","parameters":{
              "min":{"value":0.85,"unit":"1"},"mode":{"value":1.0,"unit":"1"},"max":{"value":1.15,"unit":"1"}}},
            "save_results":{"final_value":True}})
els.append({"id":"initial_damage","name":"Initial per-truck damage (age ramp)","primitive":"node",
            "value_rule":"fixed","values":[0.0,0.15,0.30,0.45,0.60,0.75],"unit":"1",
            "outputs":[{"name":"InitialDamage","unit":"1","dimensions":["Fleet"]}]})
els.append(node("is_start","First-step flag",
            b("lt",{"op":"time_ref","property":"elapsed"},
                   b("multiply",{"op":"time_ref","property":"timestep"},lit(0.5)))))
els.append(node("truck_index","Truck index",vmap(IR),dims=True))

# lags for each subsystem's damage
for s,_,_ in SUBS:
    els.append(lagnode(f"dmg_prev_{s}",f"{s} damage (prev)",f"dmg_{s}"))

# availability per subsystem (prev-based) and truck availability = product
for s,_,_ in SUBS:
    els.append(node(f"avail_{s}",f"{s} available",
        vmap(iff(b("lt",idx(f"dmg_prev_{s}"),lit(1.0)),lit(1),lit(0))),dims=True))
prod = idx(f"avail_{SUBS[0][0]}")
for s,_,_ in SUBS[1:]:
    prod = b("multiply",prod,idx(f"avail_{s}"))
els.append(node("truck_available","Truck available (all subsystems up)",vmap(prod),dims=True))

# aggregate prev damage (for wear-levelling dispatch — the FLATTENED composite metric)
tot = idx(f"dmg_prev_{SUBS[0][0]}")
for s,_,_ in SUBS[1:]:
    tot = b("add",tot,idx(f"dmg_prev_{s}"))
els.append(node("total_damage_prev","Total truck damage (prev, aggregate)",vmap(tot),dims=True))

# dispatch targets
els.append(node("target_wl","Target - wear-levelling",
    call("argmin_where",ref("total_damage_prev"),ref("truck_available"))))
els.append(node("target_cohort","Target - cohorting",
    call("argmin_where",ref("truck_index"),ref("truck_available"))))
els.append(node("target_naive","Target - naive round-robin",
    b("add",call("mod",call("floor",b("add",b("divide",
        {"op":"time_ref","property":"elapsed"},{"op":"time_ref","property":"timestep"}),lit(0.5))),lit(6)),lit(1))))
els.append(node("overload_target","Truck chosen to overload",
    iff(b("lt",ref("dispatch_policy"),lit(0.5)),ref("target_naive"),
        iff(b("lt",ref("dispatch_policy"),lit(1.5)),ref("target_wl"),ref("target_cohort"))),
    save={"time_history":True}))
els.append(node("payload","Per-truck payload",
    vmap(iff(b("eq",ref("overload_target"),IR),lit(300),lit(240))),dims=True))

# per-subsystem rate, damage recurrence, new-failure, criticality counter
for s,beta,scale in SUBS:
    els.append(node(f"rate_{s}",f"{s} damage rate",
        vmap(b("multiply",idx("truck_available"),
               b("multiply",
                 b("power",b("divide",idx("payload"),lit(240)),
                   b("multiply",lit(beta),ref("wear_mult"))),
                 lit(scale)))),dims=True))
    base = iff(b("gt",ref("is_start"),lit(0.5)),idx("initial_damage"),
               iff(b("gte",idx(f"dmg_prev_{s}"),lit(1.0)),lit(0),idx(f"dmg_prev_{s}")))
    els.append(node(f"dmg_{s}",f"{s} cumulative damage",
        vmap(b("add",base,idx(f"rate_{s}"))),dims=True,
        save={"time_history":True,"final_value":True}))
    els.append(node(f"newfail_{s}",f"{s} failure this step",
        vmap(iff(b("gte",idx(f"dmg_prev_{s}"),lit(1.0)),lit(1),lit(0))),dims=True))
    els.append(node(f"fail_step_{s}",f"{s} failures this step (fleet)",
        call("sum_array",ref(f"newfail_{s}"))))
    els.append(lagnode(f"cumfail_{s}_prev",f"{s} cum failures (prev)",f"cumfail_{s}",dims=False))
    els.append(node(f"cumfail_{s}",f"{s} cumulative failures",
        b("add",ref(f"cumfail_{s}_prev"),ref(f"fail_step_{s}")),
        save={"time_history":True,"final_value":True}))

# total failure stream = sum over subsystems
tot_fail = ref(f"fail_step_{SUBS[0][0]}")
for s,_,_ in SUBS[1:]:
    tot_fail = b("add",tot_fail,ref(f"fail_step_{s}"))
els.append(node("failures_this_step","Fleet subsystem-failures this step",tot_fail,
    save={"time_history":True}))

# shop queue (same as Stage-3d)
els.append({"id":"shop_throughput","name":"Shop throughput (repairs/step)","primitive":"node",
            "value_rule":"fixed","value":{"value":0.30,"unit":"1"},
            "description":"bays/repair_time. Higher than Stage-3d because subsystem repairs are more frequent (tires fail often); tuned to ~70% utilisation of the higher failure rate."})
els.append(lagnode("backlog_prev","Backlog (prev)","backlog",dims=False))
els.append(node("backlog","Repair backlog",
    call("max",lit(0),b("subtract",b("add",ref("backlog_prev"),ref("failures_this_step")),ref("shop_throughput"))),
    save={"time_history":True,"final_value":True}))
els.append(node("trucks_down","Repairs in queue",
    call("min",ref("backlog"),lit(6)),save={"time_history":True,"final_value":True}))
els.append(lagnode("cum_downtime_prev","Cum downtime (prev)","cum_downtime",dims=False))
els.append(node("cum_downtime","Cumulative repair-downtime (subsystem-steps)",
    b("add",ref("cum_downtime_prev"),ref("trucks_down")),
    save={"time_history":True,"final_value":True}))

model={
  "wasim_version":"0.9.7",
  "name":"Haul Fleet — Multi-Subsystem Dispatch (composite vs modal failure)",
  "description":"Stage-3e: tests whether Stage-3d's wear-levelling burstiness was an artifact of a single composite failure per truck. Each truck now fails by three INDEPENDENT subsystems with different load exponents (tires beta=4, drivetrain beta=3, structure beta=1.5), each its own Fleet-array with its own life, resetting independently on overhaul; the truck is down if ANY subsystem is down (truck_available = product of subsystem availabilities). Wear-levelling still dispatches on the FLATTENED aggregate (total_damage_prev). The failure stream is now a superposition of different-rate processes, so it should be less bursty and wear-levelling's shop-downtime penalty should shrink vs the composite Stage-3d. cumfail_{tires,drive,struct} give per-mode criticality. See STAGE3E_SUBSYSTEMS.md.",
  "source":{"generator":"gen_subsys.py"},
  "simulation_settings":{"duration":{"value":3,"unit":"yr"},"timestep":{"value":1,"unit":"wk"},
    "n_realizations":300,"sampling_method":"lhs","seed":20260729},
  "dimensions":[{"id":"Fleet","name":"Fleet","size":6,"labels":["T01","T02","T03","T04","T05","T06"]}],
  "elements":els,
}
json.dump(model,open("parameters_examples/haul_fleet_subsystems.json","w"),indent=2)
print("wrote",len(els),"elements")
