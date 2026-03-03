import SwiftCompilerPlugin
import SwiftSyntax
import SwiftSyntaxBuilder
import SwiftSyntaxMacros
import Foundation

@main
struct CoreFridayPlugin: CompilerPlugin {
    let providingMacros: [Macro.Type] = [
        GenerateStoredModelMacro.self
    ]
}

public struct GenerateStoredModelMacro: PeerMacro {
    public static func expansion(
        of node: AttributeSyntax,
        providingPeersOf declaration: some DeclSyntaxProtocol,
        in context: some MacroExpansionContext
    ) throws -> [DeclSyntax] {
        let nominal: any DeclGroupSyntax
        let typeName: String
        if let decl = declaration.as(ClassDeclSyntax.self) {
            nominal = decl
            typeName = decl.name.text
        } else if let decl = declaration.as(StructDeclSyntax.self) {
            nominal = decl
            typeName = decl.name.text
        } else {
            return []
        }

        let schemaName = explicitSchema(from: node) ?? "\(snakeCase(typeName))s"
        let storedTypeName = "Stored\(typeName)"

        let fields = extractFields(from: nominal)

        var generated: [String] = []
        generated.append("public final class \(storedTypeName): Model, @unchecked Sendable {")
        generated.append("    public static let schema = \"\(schemaName)\"")
        generated.append("")
        generated.append("    @ID(key: .id)")
        generated.append("    public var id: UUID?")

        for field in fields {
            if field.name == "id" {
                continue
            }

            generated.append("")
            let wrapper = field.isOptional ? "@OptionalField" : "@Field"
            generated.append("    \(wrapper)(key: \"\(snakeCase(field.name))\")")
            generated.append("    public var \(field.name): \(field.type)")
        }

        generated.append("")
        generated.append("    public init() {}")
        generated.append("}")

        return [DeclSyntax(stringLiteral: generated.joined(separator: "\n"))]
    }
}

private struct StoredField {
    let name: String
    let type: String
    let isOptional: Bool
}

private func extractFields(from nominal: some DeclGroupSyntax) -> [StoredField] {
    var fields: [StoredField] = []

    for member in nominal.memberBlock.members {
        guard let variable = member.decl.as(VariableDeclSyntax.self) else {
            continue
        }

        if variable.modifiers.contains(where: { $0.name.tokenKind == .keyword(.static) }) {
            continue
        }

        for binding in variable.bindings {
            guard binding.accessorBlock == nil else {
                continue
            }

            guard let identifierPattern = binding.pattern.as(IdentifierPatternSyntax.self) else {
                continue
            }

            guard let typeAnnotation = binding.typeAnnotation else {
                continue
            }

            let type = typeAnnotation.type.trimmedDescription
            let name = identifierPattern.identifier.text
            let isOptional = type.hasSuffix("?")
            fields.append(StoredField(name: name, type: type, isOptional: isOptional))
        }
    }

    return fields
}

private func explicitSchema(from node: AttributeSyntax) -> String? {
    guard let arguments = node.arguments?.as(LabeledExprListSyntax.self) else {
        return nil
    }

    for argument in arguments {
        if argument.label?.text == "schema",
            let schemaLiteral = argument.expression.as(StringLiteralExprSyntax.self),
            let segment = schemaLiteral.segments.first?.as(StringSegmentSyntax.self)
        {
            return segment.content.text
        }

        if argument.label == nil,
            let schemaLiteral = argument.expression.as(StringLiteralExprSyntax.self),
            let segment = schemaLiteral.segments.first?.as(StringSegmentSyntax.self)
        {
            return segment.content.text
        }
    }

    return nil
}

private func snakeCase(_ text: String) -> String {
    guard !text.isEmpty else {
        return text
    }

    var result = ""
    var previousWasLowercase = false

    for scalar in text.unicodeScalars {
        let character = Character(scalar)
        let isUppercase = CharacterSet.uppercaseLetters.contains(scalar)
        let isLowercase = CharacterSet.lowercaseLetters.contains(scalar)

        if isUppercase {
            if previousWasLowercase {
                result.append("_")
            }
            result.append(String(character).lowercased())
            previousWasLowercase = false
        } else {
            result.append(character)
            previousWasLowercase = isLowercase || CharacterSet.decimalDigits.contains(scalar)
        }
    }

    return result
}
