## Conclusion

The Chrono model gives the closed-form motor torque and slider position of the slider-crank. At the check time t = 1.125 s, the torque of the model is 1.4269621 N m and the closed form is 1.42695517 N m. The difference is 6.9e-6 N m, 5 % of the tolerance. The slider position of the model is 0.362258273 m, equal to the closed form to the 9 decimals of the output.

<!--curve torque.csv-->

The model agrees with the closed form at each time step, not only at the check time. The torque error is 0.26 N m after the first step, because the start accelerations of the time stepper do not agree with 60 rpm. From t = 0.05 s to the end, the error stays below 1.6e-5 N m. This is the time-step error of HHT (section "Time-step study" below).

<!--curve torque-error.csv-->

The crank turns 2.25 times, 1 turn in each second: theta = 360° × t / 1 s from the dead centre at t = 0. At t = 1.125 s, theta = 405°, so the crank is at 45 degrees, and the checked slider position is x = 0.362258 m. U1 is the move along x from the start. Thus the slider has U1 = x_slider - 0.4 m.
