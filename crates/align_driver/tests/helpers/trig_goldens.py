"""Recheck immutable trig samples using Decimal series, not native libm.

Pi uses Machin's identity; argument reduction and half-angle atan series use
600/900-digit contexts before direct integer IEEE rounding. This is sample
qualification, not a global precision theorem. Run explicitly with Python 3.
"""
from decimal import Decimal, localcontext, ROUND_HALF_EVEN
import math,struct,re
from functools import lru_cache
from pathlib import Path
from math_libm_goldens import rounded_ieee as round_ieee

def atan_small(x):
    term=x; result=x; square=-(x*x); k=3
    while True:
        term *= square
        after=result + term/k
        if after==result: return result
        result=after; k+=2

@lru_cache(maxsize=2)
def pi_value(precision):
    with localcontext() as context:
        context.prec=precision
        return 16*atan_small(Decimal(1)/5)-4*atan_small(Decimal(1)/239)

def trig(x,cosine,pi):
    tau=2*pi
    x -= (x/tau).to_integral_value(rounding=ROUND_HALF_EVEN)*tau
    term=Decimal(1) if cosine else x
    result=term; k=2 if cosine else 3
    while True:
        term *= -(x*x)/((k-1)*k)
        after=result+term
        if after==result:return result
        result=after;k+=2

def atan_positive(r):
    # Restrict the convergent series to <=tan(pi/32).
    for _ in range(3): r=r/(1+(1+r*r).sqrt())
    return 8*atan_small(r)

def atan2_finite(y,x,pi):
    a,b=abs(y),abs(x)
    if a<=b: angle=atan_positive(a/b)
    else: angle=pi/2-atan_positive(b/a)
    if x<0:angle=pi-angle
    return -angle if y<0 else angle

def bits(x,w):return int.from_bytes(struct.pack('>f' if w==32 else '>d',x),'big')
def value(raw,w):return struct.unpack('>f' if w==32 else '>d',raw.to_bytes(w//8,'big'))[0]
def expected(op,w,ys,xs,precision):
    y=value(ys,w);x=value(xs,w)
    with localcontext() as ctx:
        ctx.prec=precision
        pi=pi_value(precision); sign=ys>>(w-1)
        if op=="pair":
            a=expected("sin",w,ys,0,precision);b=expected("cos",w,ys,0,precision)
            if a is None or b is None:return None
            return round_ieee(Decimal.from_float(value(a,w))+Decimal.from_float(value(b,w)),w)
        if math.isnan(y) or (op=='atan2' and math.isnan(x)): return None
        if op in ('sin','cos'):
            if math.isinf(y): return None
            if y==0:return bits(1,w) if op=='cos' else ys
            return round_ieee(trig(Decimal.from_float(y),op=='cos',pi),w)
        if y==0:
            angle=pi if xs>>(w-1) else Decimal(0)
        elif math.isinf(y):
            angle=(pi/4 if x>0 else 3*pi/4) if math.isinf(x) else pi/2
        elif math.isinf(x):angle=Decimal(0) if x>0 else pi
        elif x==0:angle=pi/2
        else:return round_ieee(atan2_finite(Decimal.from_float(y),Decimal.from_float(x),pi),w)
        if sign:angle=angle.copy_negate()
        return round_ieee(angle,w)

def main():
    source=Path(__file__).with_name("trig_cases.rs").read_text()
    rows=re.findall(r'Sample \{ operation: "(\w+)", bits: (\d+), input: (0x[0-9a-f]+), other: (0x[0-9a-f]+), expected: (None|Some\(0x[0-9a-f]+\)) \}',source)
    assert len(rows)==293
    for operation,width,y,x,golden in rows:
        width,y,x=int(width),int(y,16),int(x,16)
        golden=None if golden=="None" else int(golden[5:-1],16)
        actual=[expected(operation,width,y,x,precision) for precision in (600,900)]
        assert actual==[golden,golden],(operation,width,hex(y),hex(x),actual,golden)
    print(f"{len(rows)} independent trigonometric sample goldens verified at 600 and 900 digits")

if __name__=="__main__":
    main()
