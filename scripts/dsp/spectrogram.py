import argparse
import numpy as np
import matplotlib.pyplot as plt
from scipy.signal import spectrogram
from scipy.io import wavfile

def main():
    parser = argparse.ArgumentParser(description="Plot a spectrogram from a WAV file")
    parser.add_argument("-p", "--path", required=True, help="Path to the WAV file")
    parser.add_argument("-o", "--out", required=True, help="Path to the output")
    args = parser.parse_args()

    print(f"Loading WAV: {args.path}")

    sr, data = wavfile.read(args.path)

    if data.ndim > 1:
        data = data[:, 0]

    f, t, Sxx = spectrogram(data, sr, nperseg=2048)

    plt.pcolormesh(t, f, 10 * np.log10(Sxx + 1e-12), shading="gouraud")
    plt.ylabel("Frequency [Hz]")
    plt.xlabel("Time [s]")
    plt.xlim(0, len(data) / sr)   # show full duration
    plt.title(f"Spectrogram of {args.path}")
    plt.colorbar(label="Power (dB)")
    plt.savefig(args.out, dpi=250)

if __name__ == "__main__":
    main()
