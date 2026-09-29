# Document Diff v0 qualification ledger

## DIF-04 two-source worker shell / runner candidate

The frozen design allows the DSI sandbox seal to be reused. The Diff runner passes two distinct read-only source descriptors (FD 3 and FD 4), a bounded request on stdin, and a private scratch directory. The trusted runner checks both raw hash/size bindings before launch; the worker checks them again before any adapter. It clears inherited environment and marks unlisted descriptors close-on-exec. Worker startup requires the DSI Linux Landlock/seccomp seal; production startup is rejected on non-Linux hosts.

Candidate profile `diff-resource-v0`: each source 256 MiB, combined sources 512 MiB, request 64 KiB, result 16 MiB, stderr 1 MiB, wall 30 s, CPU 25 s, address space 4 GiB, monitored temporary tree 2 GiB. These are **candidate bounds**, not production measurements. Node/depth/candidate/change limits are qualified in DIF-05/DIF-15.

Local macOS evidence: portable worker shell tests cover independent raw bindings, malformed/oversized request, unsupported-format unverified result, and panic containment. Strict Clippy and formatting pass. The runner rejects production construction on macOS. Linux tests for fresh process, network/exec denial, environment/descriptor isolation, timeout and output bounds are written but require the hosted Sandbox gate. No Diff format adapter or parser dependency is promoted at DIF-04.

Outstanding: hosted Linux compile/runtime, Landlock/seccomp enforcement canary, exact limits and 1-over checks, native-runtime warmup for later PDF adapter, representative large-document measurements. Until these pass, the candidate profile is not qualified for production disclosure.
