# Optional CFD controls of a report

Use these controls only when the analysis needs them.
Put them inside a `section data-cfd`.
The controls use the shared menu, segmented control, switch, value readout, and shortcut row.
Each control wrapper uses `data-cfd-control="NAME"` for its name from the table.
No `data-controls` shows every applicable control. An explicit `data-controls` shows exactly the named controls.
An empty value hides the controls. "Show all controls" temporarily shows every applicable control.
The names below join `steps`, `deformation`, and `loads` in that attribute.
Reason: each view must use the same rule for control visibility.
Reason: the flap report hid useful velocity data and did not connect its plots to the selected run.

`fh report --cfd DATA.json` embeds the source data for these controls.
The flap case writes this file through `curves.py`.
The file contains final field limits, ordered upper and lower Cp samples, coefficients, histories, and cell velocity traces.
No control fabricates absent data. Old reports retain their original controls and contour limits.
Reason: absent data must not produce a false result.

| Name in `data-controls` | HTML attribute | Use |
|---|---|---|
| `cfd-field` | Radio input `data-cfd-field`, value `p` or `U magnitude` | Choose pressure or speed in 1 action. Put the labels in a shared `segmented` component. |
| `cfd-mesh` | Checkbox `data-cfd-mesh` | Show mesh edges over a CFD contour. Omit `checked` for a clear field. |
| `cfd-flap` | Select `data-cfd-flap` | Connect the field, lift, moment, Cp, history, and flow to one solved angle. The first select supplies each option through `data-run` and `data-angle`. |
| `cfd-load` | Container `data-cfd-load="lift"` or `"moment"`, with a curve and an output | Show the selected coefficient and reference on separate scales. |
| `cfd-chord-station` | Range input `data-cfd-station`, inside `data-cfd-cp` | Show separate upper and lower −Cp values and the same station on the airfoil. The readout identifies interpolation. Use a labelled output. |
| `cfd-force-interval` | Range input `data-cfd-interval`, inside `data-cfd-history` | Show the iteration interval and the force range. Keep mesh convergence separate. |
| `cfd-streamlines` | Checkbox `data-cfd-streamlines`, with a `data-cfd-flow` figure | Show steady planar cell velocity traces from upstream seeds. A trace stops at a boundary or finite limit. |
| `cfd-flap-seed` | Checkbox `data-cfd-seed` | Add the source trace from a seed near the flap. |

A history container can contain `data-cfd-angle="ANGLE"` sections with static CSV plots and a `data-cfd-history-absent` paragraph.
Use `<!--cfd-curve NAME.csv-->` for these plots to keep full SVG samples in a small report.
Each residual plot uses initial residuals on a log axis. Absent samples form gaps.
A streamline figure states its source.
Without sufficient source data, it shows an explicit absent-data message.

The view also accepts `data-range="LOW,HIGH"`, `data-mesh="false"`, and `data-quantity="velocity"` or `"kinematic-pressure"`.
The range and quantity apply only to the first named field. The report source must justify them.
Reason: one field name alone does not establish its physical unit.
