import json
import os
import sys
import urllib.request

account = json.load(open(os.path.join(os.environ["APPDATA"], "mediaflick-verify", "accounts", sys.argv[1])))
keys = sorted(account.keys())
base = account.get("serverUrl") or account.get("server") or account.get("url")
token = account.get("accessToken") or account.get("token")
user = account.get("userId")
device = account.get("deviceId", "probe")
if not (base and token and user):
    print("unexpected account keys", keys)
    sys.exit(1)
header = f'MediaBrowser Client="MediaFlick Desktop", Device="probe", DeviceId="{device}", Version="1", Token="{token}"'


def get(path):
    req = urllib.request.Request(base.rstrip("/") + path, headers={"Authorization": header})
    with urllib.request.urlopen(req) as res:
        return json.load(res)


views = get(f"/UserViews?userId={user}")["Items"]
report = {"views": []}
seen = {}
for view in views:
    counts = {}
    start = 0
    ids = []
    while True:
        page = get(f"/Items?userId={user}&ParentId={view['Id']}&Recursive=true&Fields=ParentId&EnableImages=false&StartIndex={start}&Limit=1000")
        for item in page["Items"]:
            counts[item["Type"]] = counts.get(item["Type"], 0) + 1
            ids.append(item["Id"])
        start += len(page["Items"])
        if not page["Items"] or start >= page["TotalRecordCount"]:
            break
    for item_id in ids:
        seen.setdefault(item_id, []).append(view["Name"])
    report["views"].append({"name": view["Name"], "type": view["Type"], "collectionType": view.get("CollectionType"), "types": counts})
report["itemsInMoreThanOneView"] = sum(1 for names in seen.values() if len(names) > 1)
report["examplesInMoreThanOneView"] = [names for names in seen.values() if len(names) > 1][:5]
report["syncedTotalAcrossServer"] = get(f"/Items?userId={user}&Recursive=true&IncludeItemTypes=Movie,Series,Season,Episode&Limit=0")["TotalRecordCount"]
json.dump(report, sys.stdout, indent=1)
