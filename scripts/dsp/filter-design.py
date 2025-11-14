from __future__ import division
 
import numpy as np
from scipy import signal
import matplotlib.pyplot as plt

def plot_response(w, h, title):
    "Utility function to plot response functions"
    fig = plt.figure()
    ax = fig.add_subplot(111)
    ax.plot(w, 20*np.log10(np.abs(h)))
    ax.set_ylim(-40, 5)
    ax.grid(True)
    ax.set_xlabel('Frequency (Hz)')
    ax.set_ylabel('Gain (dB)')
    ax.set_title(title)

def main():
    fs = 96_000
    num_taps = 64
    taps = signal.remez(num_taps, bands=[0, 0.20 * fs, 0.25 * fs, 0.5 * fs], desired=[1,0], weight=[1,10],  fs=fs)

    formatted = ", ".join(f"{x:.8f}" for x in taps)
    print(f"let coeffs: [f32; {num_taps}] = [{formatted}];")

    w, h = signal.freqz(taps, [1], worN=2000, fs=fs)
    plot_response(w, h, "Low-pass Filter")
    plt.show()

if __name__ == "__main__":
    main()