# Stride Eryx patch

This directory is Eryx 0.5.0 with a blocking callback path for APIs that must
remain synchronous inside Python, such as `subprocess.run`.

The Stride `__stride_exec` callback and callbacks whose names start with
`__eryx_blocking_` are dispatched to a dedicated callback runtime and return
before the guest continues. Other Eryx callbacks retain their async behavior.
The separate runtime also prevents a blocking guest import from starving
callback execution on Eryx's current-thread runtime.

Remove this patch when Eryx provides an upstream blocking callback API.
