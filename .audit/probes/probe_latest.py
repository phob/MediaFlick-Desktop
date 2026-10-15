import json
import sys
import urllib.request

BASE = "https://demo.jellyfin.org/stable"
AUTH = 'MediaBrowser Client="probe", Device="probe", DeviceId="probe1", Version="1"'


def call(path, token=None, body=None):
    header = AUTH + (f', Token="{token}"' if token else "")
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(BASE + path, data=data, headers={"Authorization": header, "Content-Type": "application/json"})
    with urllib.request.urlopen(req) as res:
        return json.load(res)


def brief(items):
    return [f"{i['Type']}:{i['Name']}:folder={i.get('IsFolder')}:aspect={i.get('PrimaryImageAspectRatio')}" for i in items]


auth = call("/Users/AuthenticateByName", body={"Username": "demo", "Pw": ""})
token, user = auth["AccessToken"], auth["User"]["Id"]
report = {}
for view in call(f"/UserViews?userId={user}", token)["Items"]:
    vid = view["Id"]
    latest = call(f"/Items/Latest?userId={user}&ParentId={vid}&Limit=8&Fields=PrimaryImageAspectRatio", token)
    direct = call(f"/Items?userId={user}&ParentId={vid}&Fields=PrimaryImageAspectRatio&Limit=20", token)
    rand = [call(f"/Items?userId={user}&ParentId={vid}&Recursive=true&SortBy=Random&Limit=5", token)["Items"] for _ in range(2)]
    report[view["Name"]] = {
        "collectionType": view.get("CollectionType"),
        "latestIsArray": isinstance(latest, list),
        "latest": brief(latest if isinstance(latest, list) else latest.get("Items", [])),
        "directChildren": brief(direct["Items"]),
        "randomPagesDiffer": [i["Id"] for i in rand[0]] != [i["Id"] for i in rand[1]],
    }
json.dump(report, sys.stdout, indent=1)
