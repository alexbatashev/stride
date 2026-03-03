import Foundation
import Observation

@Observable
public class LangModel: @unchecked Sendable {
    public var provider: String
    public var model: String
    public var providerName: String
    public var modelName: String

    public init(provider: String, model: String, providerName: String, modelName: String) {
        self.provider = provider
        self.model = model
        self.providerName = providerName
        self.modelName = modelName
    }

    public func readableName() -> String {
        return providerName + " / " + modelName
    }
}
