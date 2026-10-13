#!/usr/bin/env python3
"""App Store Connect Analytics Reports (downloads etc.) via the API key in .env.local.

    python3 scripts/lib/asc_analytics.py list | create ONE_TIME_SNAPSHOT|ONGOING
        | reports <requestId> | instances <reportId> | segments <instanceId>

Run with the proxy variables unset: the local proxy drops most Apple API calls.
Downloads live in the COMMERCE report "App Downloads Standard" (r3-<requestId>).
"""
import os, sys, json
sys.path.insert(0, os.path.dirname(__file__))
from asc_api import Client, make_token
env={}
for l in open('.env.local'):
    if '=' in l and not l.startswith('#'):
        k,v=l.strip().split('=',1); env[k]=v.strip('"')
kid=env['ASC_KEY_ID']; iss=env['ASC_ISSUER_ID']; kp=os.path.expandvars(env['ASC_KEY_PATH'])
c=Client(make_token(kp,kid,iss))
app=c.app_id('app.solomd')
cmd=sys.argv[1]
if cmd=='list':
    r=c.get(f'/v1/apps/{app}/analyticsReportRequests')
    for x in r['data']: print(x['id'], x['attributes'])
elif cmd=='create':
    r=c.post('/v1/analyticsReportRequests', {"data":{"type":"analyticsReportRequests","attributes":{"accessType":sys.argv[2]},"relationships":{"app":{"data":{"type":"apps","id":app}}}}})
    print(r['data']['id'], r['data']['attributes'])
elif cmd=='reports':
    r=c.get(f'/v1/analyticsReportRequests/{sys.argv[2]}/reports', limit=200)
    for x in r['data']: print(x['id'], x['attributes']['category'], x['attributes']['name'])
elif cmd=='instances':
    r=c.get(f'/v1/analyticsReports/{sys.argv[2]}/instances', limit=200)
    for x in r['data']: print(x['id'], x['attributes'])
elif cmd=='segments':
    r=c.get(f'/v1/analyticsReportInstances/{sys.argv[2]}/segments')
    for x in r['data']: print(json.dumps(x['attributes']))
