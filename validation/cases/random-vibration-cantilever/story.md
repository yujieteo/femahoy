## Conclusion

The solid model gives the RMS acceleration of the tip of the cantilever within 0.06 % of the beam theory. The model gives 401161.7 mm/s2 (40.91 g), and the modal sum of the beam theory gives 400925.4 mm/s2 (40.883 g). The difference is 12 % of the tolerance of 0.5 %. Miles' equation for mode 1 alone gives 40.115 g, 1.9 % lower.

<!--curve psd.csv-->

The base input is flat at 0.04 g^2/Hz, 8.90 g RMS. At mode 1, the PSD of the tip in the model has its peak of 61.2 g^2/Hz, about 1500 times the input. Mode 1 of the solid is at 419.12 Hz, and mode 1 of the beam theory is at 417.76 Hz. Above mode 1, the PSD falls to about the input near 1170 Hz. Then it goes up toward mode 3 at 2596.8 Hz, above the band.

The band from 20 to 2000 Hz holds 2 modes of the solid, but the plot has only 1 peak. Mode 2 bends the beam in y, and the base motion in z does not move it. Mode 1 moves 61 % of the mass in z, mode 2 moves no mass in z, and mode 3 moves 19 %. The 3 modes are:

| Mode | Frequency [Hz] | Shape |
|---|---|---|
| 1 | 419.12 | first bend in z |
| 2 | 831.94 | first bend in y |
| 3 | 2596.8 | second bend in z |
