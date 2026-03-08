import Foundation

public struct ModelConfig: Codable, Identifiable, Equatable, Hashable {
    public var id: String
    public var name: String
    public var toolsSupport: Bool
    public var imageSupport: Bool
    public var thinkingSupport: Bool

    public init(
        id: String,
        name: String = "",
        toolsSupport: Bool = false,
        imageSupport: Bool = false,
        thinkingSupport: Bool = false
    ) {
        self.id = id
        self.name = name
        self.toolsSupport = toolsSupport
        self.imageSupport = imageSupport
        self.thinkingSupport = thinkingSupport
    }
}
