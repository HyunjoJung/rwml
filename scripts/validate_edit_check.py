"""Validate package-preserving edit outputs with python-docx.

- pass/: must open in python-docx AND be byte-identical (per part) to the original.
- bimg/: must open in python-docx AND contain >=1 inline image (element-tree insert).
"""
import sys, os, zipfile
from docx import Document

indir, outdir = sys.argv[1], sys.argv[2]

def parts(path):
    with zipfile.ZipFile(path) as z:
        return {n: z.read(n) for n in z.namelist()}

pass_open = pass_open_fail = pass_bytestable = pass_drift = 0

for name in sorted(os.listdir(f"{outdir}/pass")):
    p = f"{outdir}/pass/{name}"
    try:
        Document(p); pass_open += 1
    except Exception as e:
        pass_open_fail += 1; print(f"PASS-OPENFAIL {name}: {e}"); continue
    # part-payload stability vs original
    try:
        a, b = parts(f"{indir}/{name}"), parts(p)
        same = a.keys() == b.keys() and all(a[k] == b[k] for k in a)
        if same: pass_bytestable += 1
        else:
            pass_drift += 1
            diff = [k for k in a if k not in b or a[k] != b.get(k)]
            print(f"PASS-DRIFT {name}: {diff[:6]}")
    except Exception as e:
        print(f"PASS-CMP-ERR {name}: {e}")

bimg_open = bimg_open_fail = bimg_img = 0
bdir = f"{outdir}/bimg"
if os.path.isdir(bdir):
    for name in sorted(os.listdir(bdir)):
        p = f"{bdir}/{name}"
        try:
            d = Document(p); bimg_open += 1
        except Exception as e:
            bimg_open_fail += 1; print(f"BIMG-OPENFAIL {name}: {e}"); continue
        if len(d.inline_shapes) >= 1:
            bimg_img += 1
        else:
            print(f"BIMG-NOIMG {name}")

print("--- PASSTHROUGH ---")
print(f"open ok={pass_open} fail={pass_open_fail}  byte-stable={pass_bytestable} drift={pass_drift}")
print("--- TREE-EDIT IMAGE (B) ---")
print(f"open ok={bimg_open} fail={bimg_open_fail}  inline-image>=1={bimg_img}")
