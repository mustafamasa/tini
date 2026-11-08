"""Generates the Tını brand assets (SVG) into this folder. Standard library only.

    python3 docs/brand/generate.py

Geometry is defined on a 160x160 grid (the icon). The wordmark reuses the icon's t (stem, hook and 18-unit stroke) and
sits its x-height on the icon's wave line, so icon and wordmark share stroke weight, terminals and proportions.
"""
import math, sys
DARK="#1F1D2B"; CREAM="#F5F1EA"; CORAL="#F0997B"; CORAL2="#D85A30"
N=72
def centerline(dots):
    cx=72; yc=66; x0=26; x1=(116 if dots else 128)
    tc=(cx-x0)/(x1-x0)
    ph=lambda t: 2*math.pi*(1.22*t+0.07*t*t)
    off=math.pi-ph(tc)
    env=lambda t: (math.sin(math.pi*t)**1.15)*(1.0-0.12*t)
    A=15.5
    return [(x0+(x1-x0)*i/N, yc-A*env(i/N)*math.sin(ph(i/N)+off)) for i in range(N+1)]
def wave_shape(sc,ox,oy,dots,bg,col):
    c=centerline(dots)
    wdt=lambda t: 2.6+5.0*(math.sin(math.pi*t)**0.9)
    L=[];R=[]
    for i,(x,y) in enumerate(c):
        a=c[max(i-1,0)]; b=c[min(i+1,N)]
        dx,dy=b[0]-a[0],b[1]-a[1]; l=math.hypot(dx,dy) or 1
        nx,ny=-dy/l,dx/l; w=wdt(i/N)
        L.append((x+nx*w,y+ny*w)); R.append((x-nx*w,y-ny*w))
    T=lambda p: f"{ox+p[0]*sc:.1f} {oy+p[1]*sc:.1f}"
    re_=wdt(0)*sc  # end radius: both ends close with a half-circle arc, so the outline is one smooth shape
    d=("M"+" L".join(T(p) for p in L)+f" A{re_:.2f} {re_:.2f} 0 0 0 {T(R[-1])}"
       +" L"+" L".join(T(p) for p in R[::-1][1:])+f" A{re_:.2f} {re_:.2f} 0 0 0 {T(L[0])} Z")
    r0=wdt(0)
    caps="".join(f'<circle cx="{ox+p[0]*sc:.1f}" cy="{oy+p[1]*sc:.1f}" r="{r0*sc:.1f}" fill="{col}"/>' for p in (c[0],c[-1]))
    # knockout first (background-coloured outline), then the fill: works in every SVG renderer (no paint-order)
    outline=f'<path d="{d}" fill="{bg}" stroke="{bg}" stroke-width="{6*sc:.1f}" stroke-linejoin="round"/>'
    s=outline+f'<path d="{d}" fill="{col}"/>'
    if dots:
        (xa,ya),(xb,yb)=c[-5],c[-1]; dx,dy=xb-xa,yb-ya; l=math.hypot(dx,dy); dx,dy=dx/l,dy/l
        for dist,r in [(7.5,2.5),(13.5,2.0),(19,1.55)]:
            s+=f'<circle cx="{ox+(xb+dx*dist)*sc:.1f}" cy="{oy+(yb+dy*dist)*sc:.1f}" r="{r*sc:.1f}" fill="{col}"/>'
    return s
def stem(sc,ox,oy,col):
    cx=72; X=lambda v: ox+v*sc; Y=lambda v: oy+v*sc
    d=(f"M{X(cx):.1f} {Y(24):.1f} V{Y(104):.1f} C{X(cx):.1f} {Y(126):.1f} {X(cx+9):.1f} {Y(136):.1f} {X(cx+26):.1f} {Y(136):.1f} "
       f"C{X(cx+33):.1f} {Y(136):.1f} {X(cx+37):.1f} {Y(133.5):.1f} {X(cx+39.5):.1f} {Y(130):.1f}")
    return f'<path d="{d}" fill="none" stroke="{col}" stroke-width="{18*sc:.1f}" stroke-linecap="round" stroke-linejoin="round"/>'
def icon(ox,oy,size,bg=DARK,st=CREAM,wv=CORAL,dots=False,container=True):
    sc=size/160
    s=f'<rect x="{ox}" y="{oy}" width="{size}" height="{size}" rx="{37*sc:.1f}" fill="{bg}"/>' if container else ""
    return s+stem(sc,ox,oy,st)+wave_shape(sc,ox,oy,dots,bg,wv)
def wordmark_lc(ox, oy, s, col, accent=None, bg=None):
    """Lowercase 'tını' from the icon's t: same stem/hook, stroke 18*s, x-height on the icon's crossbar line."""
    import math
    sw=18*s
    st=f'fill="none" stroke="{col}" stroke-width="{sw:.2f}" stroke-linecap="round" stroke-linejoin="round"'
    X=lambda v: ox+v*s; Y=lambda v: oy+v*s
    cx=24; top, xh, base = 30, 66, 136
    out=[f'<path d="M{X(cx):.1f} {Y(top):.1f} V{Y(104):.1f} C{X(cx):.1f} {Y(126):.1f} {X(cx+9):.1f} {Y(base):.1f} {X(cx+26):.1f} {Y(base):.1f} C{X(cx+33):.1f} {Y(base):.1f} {X(cx+37):.1f} {Y(133.5):.1f} {X(cx+39.5):.1f} {Y(130):.1f}" {st}/>']
    if accent:
        pts=" L".join(f"{X(cx-26+54*i/48):.1f} {Y(xh-7.5*math.sin(2*math.pi*i/48+math.pi*0.0)*math.sin(math.pi*i/48)**0.6):.1f}" for i in range(49))
        out.append(f'<path d="M{pts}" fill="none" stroke="{bg}" stroke-width="{sw*0.8+6*s:.2f}" stroke-linecap="round" stroke-linejoin="round"/>')
        out.append(f'<path d="M{pts}" fill="none" stroke="{accent}" stroke-width="{sw*0.8:.2f}" stroke-linecap="round" stroke-linejoin="round"/>')
    else:
        out.append(f'<path d="M{X(cx-20):.1f} {Y(xh):.1f} H{X(cx+22):.1f}" {st}/>')
    x=cx+70
    out.append(f'<path d="M{X(x):.1f} {Y(xh):.1f} V{Y(base):.1f}" {st}/>')
    x+=44; r=26
    out.append(f'<path d="M{X(x):.1f} {Y(base):.1f} V{Y(xh+r):.1f} A{r*s:.1f} {r*s:.1f} 0 0 1 {X(x+2*r):.1f} {Y(xh+r):.1f} V{Y(base):.1f}" {st}/>')
    x+=2*r+44
    out.append(f'<path d="M{X(x):.1f} {Y(xh):.1f} V{Y(base):.1f}" {st}/>')
    return "".join(out), (x+9)*s


def svg(w, h, body, bg=None):
    rect = f'<rect width="{w}" height="{h}" fill="{bg}"/>' if bg else ""
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">{rect}{body}</svg>\n'


def app_icon(dots=False):
    """1024x1024 macOS app icon: 824 squircle with the system padding."""
    size, pad = 824, 100
    return svg(1024, 1024, icon(pad, pad, size, dots=dots))


def main():
    import os
    out = os.path.dirname(os.path.abspath(__file__))
    files = {
        "tini-icon.svg": app_icon(),
        "tini-icon-sampling.svg": app_icon(dots=True),
        "tini-mark-on-dark.svg": svg(160, 160, stem(1, 0, 0, CREAM) + wave_shape(1, 0, 0, False, DARK, CORAL), DARK),
        "tini-mark-on-light.svg": svg(160, 160, stem(1, 0, 0, DARK) + wave_shape(1, 0, 0, False, CREAM, CORAL2), CREAM),
        "tini-wordmark-dark.svg": svg(300, 150, wordmark_lc(0, -10, 1.0, DARK)[0]),
        "tini-wordmark-light.svg": svg(300, 150, wordmark_lc(0, -10, 1.0, CREAM)[0]),
        "tini-lockup-dark.svg": svg(560, 200, icon(30, 30, 140) + wordmark_lc(208, 20, 0.86, CREAM)[0], DARK),
        "tini-lockup-light.svg": svg(560, 200, icon(30, 30, 140) + wordmark_lc(208, 20, 0.86, DARK)[0], CREAM),
    }
    for name, content in files.items():
        with open(os.path.join(out, name), "w") as f:
            f.write(content)
    print("\n".join(files))


if __name__ == "__main__":
    main()
