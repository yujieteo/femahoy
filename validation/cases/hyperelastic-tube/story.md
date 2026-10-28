## Conclusion

The CalculiX model inflates the Ogden rubber tube past its pressure maximum. At each of its 40 increments, the pressure of the model is within 0.018 % of the exact pressure of Haughton and Ogden (1979). The tolerance of the checks is 0.2 %.

The exact pressure has its maximum 0.229790 MPa at the inner stretch λ_a = a/A = 2.566. After the maximum, the pressure decreases, and the model follows it to λ_a = 4.0. A pressure load cannot pass the maximum, thus the model moves the inner face and gets the pressure from the reaction forces.

<!--curve pressure.csv-->

The model is the quarter tube, through 40 increments. The inner radius grows from 10 mm to 40 mm. Of the 40 increments, p is largest at λ_a = 2.575, the increment nearest the maximum. After it, p decreases. At λ_a = 4.0, the displacement of the outer face is u(B) = 23.590 mm, and the exact value is 23.589 mm. The pressure p of the model at each increment is:

| λ_a | p [MPa] |
|---|---|
| 1.075 | 0.04216 |
| 1.150 | 0.07667 |
| 1.225 | 0.10505 |
| 1.300 | 0.12845 |
| 1.375 | 0.14779 |
| 1.450 | 0.16379 |
| 1.525 | 0.17701 |
| 1.600 | 0.18793 |
| 1.675 | 0.19692 |
| 1.750 | 0.20429 |
| 1.825 | 0.21031 |
| 1.900 | 0.21519 |
| 1.975 | 0.21910 |
| 2.050 | 0.22221 |
| 2.125 | 0.22463 |
| 2.200 | 0.22647 |
| 2.275 | 0.22782 |
| 2.350 | 0.22877 |
| 2.425 | 0.22937 |
| 2.500 | 0.22969, check `f_25` |
| 2.575 | 0.22976, the largest p |
| 2.650 | 0.22965 |
| 2.725 | 0.22938 |
| 2.800 | 0.22897 |
| 2.875 | 0.22847 |
| 2.950 | 0.22788 |
| 3.025 | 0.22723 |
| 3.100 | 0.22655 |
| 3.175 | 0.22583 |
| 3.250 | 0.22510 |
| 3.325 | 0.22437 |
| 3.400 | 0.22365 |
| 3.475 | 0.22295 |
| 3.550 | 0.22226 |
| 3.625 | 0.22161 |
| 3.700 | 0.22100 |
| 3.775 | 0.22043 |
| 3.850 | 0.21990 |
| 3.925 | 0.21942 |
| 4.000 | 0.21900, checks `f_40` and `u_outer` |

The checks find a wrong rubber model. A Neo-Hooke rubber with the same shear modulus at small strain gives a force 14.2 % too large at λ_a = 2.5. At λ_a = 4.0, the force is 28.8 % too large (see "Wrong models").
