# ABI Reference

The long-term contract for Greentic MCP tools is the component world described
in [`greentic-interfaces`](https://docs.rs/greentic-interfaces). Hosts can call
the generated bindings to invoke the `component-api::invoke` function and work
directly with `TenantCtx` and `Outcome<T>` structs from `greentic-types`.

Until every tool is published as a component, the executor ships a fallback ABI
that mirrors the minimal pointer/length pattern used by many existing WASI
tools. When no component entry is found the executor expects the following core
WebAssembly export:

```
(func (export "tool_invoke")
      (param i32 i32)  ;; (in_ptr, in_len)
      (result i32 i32) ;; (out_ptr, out_len)
)
```

- The host copies UTF-8 encoded JSON input into guest memory and passes the
  pointer/length pair to `tool_invoke`.
- The guest must return a pointer/length pair describing UTF-8 JSON output that
  stays valid until the next invocation (leaking the allocation is acceptable in
  short-lived tools).
- If the guest traps, the host classifies the failure as transient and retries
  according to the configured policy.

When the component export is available the executor will be upgraded to prefer
the generated bindings and this file will be revised to link to the precise
binding paths.
