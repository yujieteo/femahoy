## Conclusion

The CalculiX model inflates the Ogden rubber tube past its pressure maximum. At each of its 40 increments, the pressure of the model is within 0.018 % of the exact pressure of Haughton and Ogden (1979). The tolerance of the checks is 0.2 %.

The exact pressure has its maximum 0.229790 MPa at the inner stretch λ_a = a/A = 2.566. After the maximum, the pressure decreases, and the model follows it to λ_a = 4.0. A pressure load cannot pass the maximum, thus the model moves the inner face and gets the pressure from the reaction forces.

<!--curve pressure.csv-->

The model view shows the true shape of the quarter tube, with the magnitude of the displacement U. Move the inflation slider above the view through the 40 increments. The inner radius grows from 10 mm to 40 mm. The slider gives λ_a and the pressure p of the model at each increment. Of the 40 increments, p is largest at λ_a = 2.575, the increment nearest the maximum. After it, p decreases. At λ_a = 4.0, the mark gives the displacement u(B) of the outer face.

The checks find a wrong rubber model. A Neo-Hooke rubber with the same shear modulus at small strain gives a force 14.2 % too large at λ_a = 2.5. At λ_a = 4.0, the force is 28.8 % too large (see "Wrong models").
