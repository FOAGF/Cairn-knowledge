"""Usage: python measure.py   (spawns fresh interpreters; prints import time / RSS)"""
import subprocess, sys, json
CODE = r'''
import time, os, sys, json
def rss():
    for l in open('/proc/self/status'):
        if l.startswith('VmRSS'): return int(l.split()[1])/1024
res={"rss_baseline":rss()}
t=time.perf_counter(); import litellm; res["import_s"]=time.perf_counter()-t; res["rss_after_import"]=rss(); res["modules"]=len(sys.modules)
import mock_server
srv,st,url=mock_server.start()
m=[{"role":"user","content":"hi"}]
t=time.perf_counter(); litellm.completion(model="openai/x",messages=m,api_base=url+"/v1",api_key="k"); res["first_call_s"]=time.perf_counter()-t
t=time.perf_counter(); litellm.completion(model="openai/x",messages=m,api_base=url+"/v1",api_key="k"); res["second_call_s"]=time.perf_counter()-t
res["rss_after_openai_call"]=rss()
litellm.completion(model="anthropic/claude-sonnet-4-5",messages=m,api_base=url,api_key="k")
litellm.completion(model="azure/x",messages=m,api_base=url,api_key="k",api_version="2024-10-21")
litellm.responses(model="openai/x",input="hi",api_base=url+"/v1",api_key="k")
res["rss_after_all_formats"]=rss()
import azure.identity; res["rss_after_azure_identity"]=rss()
print(json.dumps(res))
'''
def run(env_extra):
    import os
    env = dict(os.environ, **env_extra)
    out = subprocess.run(["nice","-n","10",sys.executable,"-c",CODE],capture_output=True,text=True,env=env)
    return json.loads(out.stdout.strip().splitlines()[-1]) if out.stdout.strip() else out.stderr[-500:]
for label, env in [("default env", {}), ("LITELLM_LOCAL_MODEL_COST_MAP=True", {"LITELLM_LOCAL_MODEL_COST_MAP":"True"})]:
    for i in range(3):
        r=run(env); print(label, {k:round(v,2) for k,v in r.items()} if isinstance(r,dict) else r)
