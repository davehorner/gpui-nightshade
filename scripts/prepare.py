"""Initialize only the pinned build dependencies and apply the integration patch."""
from pathlib import Path
import subprocess
ROOT=Path(__file__).resolve().parents[1]
def prepare():
    for name in ['nightshade','gpui-component']:
        path=ROOT/'upstream'/name
        if not (path/'Cargo.toml').is_file():
            subprocess.run(['git','submodule','update','--init','--depth','1','--','upstream/'+name],cwd=ROOT,check=True)
    checkout=ROOT/'upstream/nightshade';patch=ROOT/'patches/nightshade/combined.patch'
    check=subprocess.run(['git','apply','--check',str(patch)],cwd=checkout,capture_output=True)
    if check.returncode==0:subprocess.run(['git','apply',str(patch)],cwd=checkout,check=True)
    else:
        reverse=subprocess.run(['git','apply','--reverse','--check',str(patch)],cwd=checkout,capture_output=True)
        if reverse.returncode:raise SystemExit('Nightshade integration patch conflicts with local source changes')
if __name__=='__main__':prepare()
