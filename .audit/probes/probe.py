import json
import sys
import urllib.request

BASE = sys.argv[1] if len(sys.argv) > 1 else "https://demo.jellyfin.org/stable"
AUTH = 'MediaBrowser Client="probe", Device="probe", DeviceId="probe1", Version="1"'


def call(path, token=None, body=None):
    header = AUTH + (f', Token="{token}"' if token else "")
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(BASE + path, data=data, headers={"Authorization": header, "Content-Type": "application/json"})
    with urllib.request.urlopen(req) as res:
        return json.load(res)


auth = call("/Users/AuthenticateByName", body={"Username": "demo", "Pw": ""})
token, user = auth["AccessToken"], auth["User"]["Id"]
views = call(f"/UserViews?userId={user}", token)["Items"]
report = {"server": BASE, "views": []}
for view in views:
    counts = {}
    page = call(f"/Items?userId={user}&ParentId={view['Id']}&Recursive=true&Fields=ParentId&Limit=2000", token)
    for item in page["Items"]:
        counts[item["Type"]] = counts.get(item["Type"], 0) + 1
    top = [i for i in page["Items"] if i["Type"] in ("Movie", "Series", "Video", "MusicAlbum")]
    direct_parent = sum(1 for i in top if i.get("ParentId") == view["Id"])
    report["views"].append({
        "id": view["Id"], "name": view["Name"], "type": view["Type"],
        "collectionType": view.get("CollectionType"),
        "total": page["TotalRecordCount"], "types": counts,
        "topLevel": len(top), "topLevelParentIsView": direct_parent,
    })
everything = call(f"/Items?userId={user}&Recursive=true&IncludeItemTypes=Movie,Series,Season,Episode&Limit=0", token)
report["syncedTotalAcrossServer"] = everything["TotalRecordCount"]
json.dump(report, sys.stdout, indent=1)
