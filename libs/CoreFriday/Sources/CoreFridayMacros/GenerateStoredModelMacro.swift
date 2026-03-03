@attached(peer, names: prefixed(Stored))
public macro GenerateStoredModel(schema: String? = nil) = #externalMacro(
    module: "CoreFridayMacroPlugin",
    type: "GenerateStoredModelMacro"
)
