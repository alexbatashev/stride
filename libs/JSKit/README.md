# JSKit + QuickJS-NG

`JSKit` embeds QuickJS-NG as source and builds it as part of SwiftPM.

## Source checkout

QuickJS-NG is tracked as a git submodule at:

- `Vendor/quickjs-ng`

Clone with submodules:

```bash
git clone --recurse-submodules <repo>
```

or after clone:

```bash
git submodule update --init --recursive
```

## How it builds

- SwiftPM target `CQuickJS` compiles upstream QuickJS source files from `Vendor/quickjs-ng`.
- `JSKit` is a Swift wrapper over `CQuickJS`.
- No system package (`brew/apt`) is required.

## API

```swift
let runtime = try JavaScriptRuntime()
let context = try runtime.makeContext()
let value = try context.evaluate("1 + 2")
print(try value.string()) // 3
```
