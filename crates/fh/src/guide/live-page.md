# Optional controls of the live page

The live page uses named controls from the shared UI layer. Its `data-controls` attribute on `body` selects only the named optional controls.
The Runs list is page navigation, not an optional control. It is always on the page.
For example, `&controls=run-search,files,mesh-compare` shows the controls for a mesh study.
The `controls` URL parameter sets that attribute for a live page link.
Without `data-controls`, the page offers every applicable control. With `data-controls`, it offers exactly the named optional controls.
An empty attribute hides every optional control. These page controls apply to a live chat, not to a static report.
Keep the `token` and `run` parameters.

| Name | Use |
| --- | --- |
| `command-state` | Show Active, Done, Failed, or Stopped, with the elapsed command time. Use it for a solve or a report command. |
| `prepare-rerun` | After a stop, fill the composer with a request for the retained input of the selected run. The person must send the request. |
| `files` | Offer the retained input, result files, and the latest registered report of the selected run. Use it when the person must inspect the input or report. |
| `run-search` | Show a search box above the Runs list for run IDs and source paths. Use it when an analysis has several runs. |
| `mesh-compare` | Choose mesh A and mesh B from Runs. Show undeformed meshes with a linked camera and zoom. Use it to inspect mesh changes. |

A mesh pair gives no convergence result. It compares no contour field, unit, time step, or independent deformation scale.
A load error names its side. The other side stays open. Uncheck Link views for separate camera poses.
The report command retains a report copy for each named run. Open report uses a sandbox with no daemon access.
Reason: scout runs R5 to R9 showed repeated run names, hidden report paths, and unclear command and stop state.
