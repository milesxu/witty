# Native OpenGL Xvfb Probe Result

Updated: 2026-06-01

`m320-native-opengl-xvfb-probe-when-approved` ran the guarded Xvfb/software-GL
window probe:

```text
WITTY_CAPTURE_MODE=xvfb scripts/capture-gui-diagnostics.sh target/gui-regression/opengl-xvfb.xwd
```

The probe did not start Chromium and did not enable Vulkan. The startup report
confirmed the intended Witty policy before renderer initialization:

```json
{
  "chromium": false,
  "event": "witty.native_window_startup",
  "honors_wgpu_backend_env": false,
  "native_backend_policy": "gl",
  "opengl_only": true,
  "renderer": "wgpu",
  "vulkan_enabled_by_witty": false,
  "will_request_adapter": true
}
```

Result: blocked before screenshot capture. `wgpu` could not create a GL surface
inside Xvfb:

```text
libEGL warning: DRI3: not supported
libEGL warning: DRI2: failed to authenticate
failed to initialize wgpu renderer ... failed to create wgpu surface: Failed to create surface for any enabled backend: {}
```

Artifact:

```text
target/gui-regression/opengl-xvfb.xwd.startup.log
```

Interpretation:

- This is not evidence of Vulkan use; Witty reported OpenGL-only policy.
- This is not a Chromium/WebGPU issue; no browser was launched.
- The current blocker is Xvfb/EGL surface creation for `wgpu` GL on this host.
- Do not keep retrying the same local Xvfb probe without changing the GL/EGL
  environment or moving the real native window probe to a safer platform.

Recommended next step: keep local routine validation on non-graphical policy
checks and pure Rust tests. For real native window validation, use a machine
where Warp/OpenGL is already known to run, such as `aibookmx`, or investigate a
surfaceless/headless renderer harness that avoids Xvfb surface creation.
