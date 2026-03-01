import Foundation
import Observation

@Observable
class LangModel {
    var provider: String
    var model: String
    var providerName: String
    var modelName: String
    
    init(provider: String, model: String, providerName: String, modelName: String) {
        self.provider = provider
        self.model = model
        self.providerName = providerName
        self.modelName = modelName
    }
    
    func readableName() -> String {
        return providerName + " / " + modelName
    }
}
