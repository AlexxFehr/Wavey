"""Find zero-thickness panels in a room mesh (faces with air on both sides)
and give them a thickness, so ray-parity inside tests and voxelisers see a
closed room. Air = voxels reachable from inside the shell without crossing
a surface (flood fill from the exterior, everything else not touched by a
surface is air)."""
import numpy as np, sys, collections
from scipy import ndimage
path=sys.argv[1]; h=0.04; dz=0.02
lines=open(path).read().splitlines()
V=[];F=[];G=[];cur=None;fidx=[]
for i,l in enumerate(lines):
    s=l.split()
    if not s: continue
    if s[0]=='v': V.append(list(map(float,s[1:])))
    if s[0]=='usemtl': cur=s[1]
    if s[0]=='f': F.append([int(x)-1 for x in s[1:]]);G.append(cur);fidx.append(i)
V=np.array(V);F=np.array(F)
lo=V.min(0)-3*h; hi=V.max(0)+3*h; n=np.ceil((hi-lo)/h).astype(int)+1
wall=np.zeros(n,bool)
for f in F:
    a,b,c=V[f]; L=max(np.linalg.norm(b-a),np.linalg.norm(c-a),np.linalg.norm(c-b))
    k=int(L/(h*0.4))+2
    u,v=np.meshgrid(np.linspace(0,1,k),np.linspace(0,1,k)); m=u+v<=1
    P=a+np.outer(u[m],b-a)+np.outer(v[m],c-a)
    idx=np.floor((P-lo)/h).astype(int); wall[idx[:,0],idx[:,1],idx[:,2]]=True
lab,_=ndimage.label(~wall)
ext=lab[0,0,0]
air=(lab!=ext)&(~wall)
print('air volume ≈ %.1f m³ (+ half the surface voxels ≈ %.1f)'%(air.sum()*h**3, (air.sum()+0.5*wall.sum())*h**3))
def isair(p):
    i=np.floor((p-lo)/h).astype(int)
    return air[i[0],i[1],i[2]]
A=V[F[:,0]];B=V[F[:,1]];C=V[F[:,2]]
N=np.cross(B-A,C-A); ar=np.linalg.norm(N,axis=1); ok=ar>1e-10; N[ok]/=ar[ok][:,None]
thin=np.zeros(len(F),bool); side=np.zeros(len(F))
for i in np.where(ok)[0]:
    votes=[]
    for (u,v) in [(1/3,1/3),(0.6,0.2),(0.2,0.6),(0.2,0.2)]:
        p=A[i]+u*(B[i]-A[i])+v*(C[i]-A[i])
        votes.append((isair(p+N[i]*2.5*h),isair(p-N[i]*2.5*h)))
    both=sum(a and b for a,b in votes); plus=sum(a and not b for a,b in votes); minus=sum(b and not a for a,b in votes)
    if both>=3: thin[i]=True
    side[i]=1 if plus>minus else (-1 if minus>plus else 0)
print('thin triangles',thin.sum(),'area %.1f m²'%(ar[thin].sum()/2), collections.Counter(G[i] for i in np.where(thin)[0]))
# orient: normals into the air
flip=0
for i in np.where(ok&~thin)[0]:
    if side[i]<0: F[i]=F[i][[0,2,1]]; flip+=1
print('flipped',flip,'undetermined',int((ok&~thin&(side==0)).sum()))
# thicken thin faces: offset copies along the face normal, per connected panel
adds=collections.defaultdict(list); newV=[]
tset=np.where(thin)[0]
# connected components by shared vertices
parent=list(range(len(tset)))
def fnd(x):
    while parent[x]!=x: parent[x]=parent[parent[x]]; x=parent[x]
    return x
byv=collections.defaultdict(list)
for k,i in enumerate(tset):
    for v in F[i]: byv[v].append(k)
for ks in byv.values():
    for k in ks[1:]: parent[fnd(k)]=fnd(ks[0])
comps=collections.defaultdict(list)
for k,i in enumerate(tset): comps[fnd(k)].append(i)
nv=len(V)
for comp in comps.values():
    # consistent normal: orient all like the first (coplanar-ish panels)
    n0=N[comp[0]]
    for i in comp:
        if N[i]@n0<0: F[i]=F[i][[0,2,1]]; N[i]=-N[i]
    vn=collections.defaultdict(lambda: np.zeros(3))
    for i in comp:
        for v in F[i]: vn[v]+=N[i]
    top={}
    for v,nn in vn.items():
        top[v]=nv+len(newV); newV.append(V[v]+nn/np.linalg.norm(nn)*dz)
    cnt=collections.Counter()
    for i in comp:
        for k in range(3): cnt[(F[i][k],F[i][(k+1)%3])]+=1
    for i in comp:
        g=G[i]; a,b,c=F[i]
        adds[g].append((top[a],top[c],top[b]))
        for k in range(3):
            x,y=F[i][k],F[i][(k+1)%3]
            if cnt[(y,x)]==0:  # boundary edge
                adds[g].append((x,y,top[y])); adds[g].append((x,top[y],top[x]))
for k,i in enumerate(fidx): lines[i]='f %d %d %d'%tuple(F[k]+1)
vl=max(i for i,l in enumerate(lines) if l.startswith('v '))
lines=lines[:vl+1]+['v %.5f %.5f %.5f'%tuple(p) for p in newV]+lines[vl+1:]
for g,ts in adds.items():
    lines+=['g %s_thick'%g,'usemtl %s'%g]+['f %d %d %d'%(a+1,b+1,c+1) for a,b,c in ts]
open(path,'w').write('\n'.join(lines)+'\n')
print('panels thickened',len(comps))
