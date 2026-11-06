from pathlib import Path
import re, subprocess, os, sys

project=Path(sys.argv[1]).resolve()
version=sys.argv[2]
cache=Path.home()/'.gradle/caches/fabric-loom'
if version.startswith('26.'):
    jars=[cache/version/'minecraft-client.jar']
else:
    jars=list((cache/'minecraftMaven/net/minecraft/minecraft-merged').glob(version+'-*/*.jar'))
    if not jars:
        jars=list((cache/'minecraftMaven/net/minecraft/minecraft-clientonly').glob(version+'-*/*.jar'))
assert jars, f'No Minecraft compile jar for {version}'
classpath=os.pathsep.join(map(str,jars))
javap=Path(os.environ['JAVA_HOME'])/'bin'/('javap.exe' if os.name=='nt' else 'javap')
failures=[]
checked=0
for source in (project/'src').rglob('*.java'):
    text=source.read_text(encoding='utf-8-sig')
    target=re.search(r'@Mixin\((\w+)\.class\)',text)
    if not target: continue
    imported=re.search(r'import\s+([\w.]+\.'+target[1]+r');',text)
    assert imported, f'Missing Mixin import in {source}'
    out=subprocess.check_output([str(javap),'-classpath',classpath,'-p','-s',imported[1]],text=True,encoding='utf-8',errors='replace')
    available=[]
    for match in re.finditer(r'\s([\w$<>]+)\([^\n]*\);\s*descriptor:\s*(\S+)',out):
        available.append(match[1]+match[2])
    for invoker in re.finditer(r'@Invoker\("([^"]+)"\)',text):
        checked+=1
        if not any(x.startswith(invoker[1]+'(') for x in available):
            failures.append(f'{source.name}: {imported[1]} has no invoker target {invoker[1]}')
    for annotation in re.finditer(r'@(Inject|ModifyVariable|Redirect|ModifyArg)\s*\(([\s\S]*?)\)\s*(?:\r?\n)',text):
        spec=re.search(r'method\s*=\s*("[^"]+"|\{[^}]+\})',annotation[2])
        if not spec: continue
        for method in re.findall(r'"([^"]+)"',spec[1]):
            checked+=1
            found=method in available if '(' in method else any(x.startswith(method+'(') for x in available)
            if not found: failures.append(f'{source.name}: {imported[1]} has no method {method}')
if failures:
    print('\n'.join(failures))
    sys.exit(1)
print(f'PASS {version}: {checked} Mixin targets exist in Minecraft bytecode')
