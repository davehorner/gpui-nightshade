"""Build the real Nightshade viewer for static hosting (including project Pages paths)."""
from pathlib import Path
import argparse,os,shutil,subprocess,tomllib,sys,hashlib
ROOT=Path(__file__).resolve().parents[1]
def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--out',type=Path,default=ROOT/'target/nightshade-web');args=parser.parse_args();args.out=args.out.resolve()
    subprocess.run([sys.executable,str(ROOT/'scripts/prepare.py')],check=True)
    upstream=ROOT/'upstream/nightshade'
    subprocess.run(['cargo','+nightly-2026-10-31','rustc','--manifest-path',str(ROOT/'Cargo.toml'),'-p','gpui-nightshade-viewer','--lib','--target','wasm32-unknown-unknown','--profile','web','--locked','--','-C','link-arg=-zstack-size=8388608'],check=True)
    version=next(p['version'] for p in tomllib.loads((ROOT/'Cargo.lock').read_text(encoding='utf-8'))['package'] if p['name']=='wasm-bindgen')
    local=ROOT/'target/wasm-tools/bin'/('wasm-bindgen.exe' if os.name=='nt' else 'wasm-bindgen');tool=str(local) if local.exists() else shutil.which('wasm-bindgen')
    if not tool or subprocess.check_output([tool,'--version'],text=True).strip()!=f'wasm-bindgen {version}':raise SystemExit(f'Install wasm-bindgen-cli {version} into target/wasm-tools')
    token=hashlib.sha256((ROOT/"target/wasm32-unknown-unknown/web/gpui_nightshade_web.wasm").read_bytes()).hexdigest()[:16]
    args.out.mkdir(parents=True,exist_ok=True)
    subprocess.run([tool,str(ROOT/'target/wasm32-unknown-unknown/web/gpui_nightshade_web.wasm'),'--target','web','--out-dir',str(args.out/'pkg'/token),'--out-name','nightshade','--no-typescript'],check=True)
    
    for file in (ROOT/'web').iterdir():
        if file.suffix in {'.html','.js'}:(args.out/file.name).write_text(file.read_text(encoding='utf-8').replace('__BUILD_ID__',token),encoding='utf-8',newline='\n')
    (args.out/'.nojekyll').touch()
    shutil.copyfile(upstream/'crates/plugins/describe/nightshade-platform/src/images/icon.png',args.out/'icon.png')
    print(f'Built {args.out}')
if __name__=='__main__':main()
