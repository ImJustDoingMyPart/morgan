#!/usr/bin/env python3
"""Ambiente para rellenar los tramos sin voz de `morgan about` (dev/acerca-clip).

Ruido con el espectro promedio del 20 % más silencioso del tramo: en todo el video no hay
medio segundo de ambiente sin voz ni ladridos, y un ruido sintético no puede traer ninguno.
Semilla fija: el mismo video da el mismo archivo.

uso: acerca-ambiente.py VIDEO INICIO DURACIÓN SALIDA.wav
"""
import sys, subprocess, wave, numpy as np
src, ini, dur, out = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
sr=48000
pcm=subprocess.run(["ffmpeg","-v","error","-ss",ini,"-t",dur,"-i",src,"-vn","-ac","2","-ar",str(sr),"-f","s16le","-"],capture_output=True).stdout
x=np.frombuffer(pcm,np.int16).astype(float).reshape(-1,2)
n=2048; hop=n//2; win=np.hanning(n)
frames=[x[i:i+n] for i in range(0,len(x)-n,hop)]
e=np.array([(f**2).mean() for f in frames])
quietos=[f for f,v in zip(frames,e) if v<=np.percentile(e,20)]
P=np.mean([np.abs(np.fft.rfft(f*win[:,None],axis=0))**2 for f in quietos],axis=0)
rms=np.sqrt(np.mean([(f**2).mean() for f in quietos]))
rng=np.random.default_rng(1)
total=int(float(dur)*sr); y=np.zeros((total+n,2))
for i in range(0,total,hop):
    fase=np.exp(2j*np.pi*rng.random(P.shape))
    y[i:i+n]+=np.fft.irfft(np.sqrt(P)*fase,n=n,axis=0)*win[:,None]
y=y[:total]; y*=rms/np.sqrt((y**2).mean())
with wave.open(out,"wb") as w:
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(sr)
    w.writeframes(np.clip(y,-32768,32767).astype(np.int16).tobytes())
print("quietos", len(quietos), "rms %.1f dBFS"%(20*np.log10(rms/32768)))
