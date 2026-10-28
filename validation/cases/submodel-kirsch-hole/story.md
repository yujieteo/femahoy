## Conclusion

The submodel gives σ_xx at the hole 0.029 % above the Kirsch solution (1898). The coarse global model alone gives the peak 4.5 % low. The check is the largest σ_xx at the integration points of the element `TOP`, at the top of the hole. There the submodel gives 296.265 MPa, and the Kirsch solution gives 296.179 MPa. The tolerance is 0.1 %.

The global model has 16 elements in 1/4 of the plate. The submodel is the ring from the hole to the cut at r = 2a = 20 mm, with 288 elements. It gets the displacement of the global model on the cut, and it has no other load. `fh run job.inp` solves `global.inp` first, then `job.inp` in the same folder.

The model view shows σ_xx (`S11`) in the submodel, from the top. Choose a view above the model:

- The submodel, σ_xx: the stress is highest at the top of the hole, 3 times the tension of 100 MPa. The marks give the nodal S11 at the hole and at the cut. The nodal value is the mean of the elements at the node, 0.14 % above 300 MPa at the hole.
- The top of the hole: the mark is at the integration point of the check, in the element `TOP`.

On the line x = 0, the global model is 4.5 % low at the hole and 8 % high at its second node, y = 12.46 mm. The submodel follows the Kirsch solution within 0.4 % from the hole to y = 18 mm:

<!--curve line-x0.csv-->

Near the cut, the submodel has the error of the global model: -1.0 % at y = 19.06 mm and -3.3 % at the cut. Thus read the results of a submodel away from its cut.

The error of 0.029 % comes from the displacement of the global model on the cut. With the exact Kirsch displacement on the cut, the same submodel gives +0.000 %. A finer global mesh makes the error smaller: 64 elements give +0.016 %, and 256 elements give +0.000 %. An error of the method gives a larger difference. A cut at r = 1.5a gives -1.0 %, and a global model with 6 elements gives -1.7 % (see "Studies").
