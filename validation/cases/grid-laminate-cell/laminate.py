"""The exact effective material of the laminate cell of grid-laminate-cell (derivation.md)."""

f=[0.3,0.7]; E=[70e3,3e3]; nu=[0.33,0.38]; a=[23e-6,70e-6]; k=[200,0.25]
m=lambda v:sum(fi*vi for fi,vi in zip(f,v))
print("kxx %.10g kzz %.10g"%(m(k),1/m([1/x for x in k])))
lam=[e*n/((1+n)*(1-2*n)) for e,n in zip(E,nu)]; G=[e/(2*(1+n)) for e,n in zip(E,nu)]
C11=[l+2*g for l,g in zip(lam,G)]
s33=m([1/c for c in C11]); r=m([l/c for l,c in zip(lam,C11)])
print("c33 %.10g c13 %.10g"%(1/s33,r/s33))
print("c11 %.10g"%(m([c-l*l/c for c,l in zip(C11,lam)])+r*r/s33))
print("c12 %.10g"%(m([l-l*l/c for c,l in zip(C11,lam)])+r*r/s33))
print("c44 %.10g c66 %.10g"%(1/m([1/g for g in G]),m(G)))
B=[e/(1-n) for e,n in zip(E,nu)]; eps=m([b*x for b,x in zip(B,a)])/m(B)
az=m([x*(1+n)/(1-n) for x,n in zip(a,nu)])-eps*m([2*n/(1-n) for n in nu])
print("alphaxx %.10g alphazz %.10g"%(eps,az))
print("density %.10g"%m([2.7e-9,1.2e-9]), "cp %.10g"%(m([2.7e-9*9e8,1.2e-9*1.5e9])/m([2.7e-9,1.2e-9])))
