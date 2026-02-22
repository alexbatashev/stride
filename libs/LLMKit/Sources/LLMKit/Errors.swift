import Foundation

public enum LLMError: Error, Equatable, Sendable {
    case requestError(String)
    case parsingError(String)
    case serverError(Int)
    case tlsError(String)
    case invalidRequest(String)
    case unknown
}

extension LLMError: LocalizedError {
    public var errorDescription: String? {
        switch self {
        case .requestError(let message):
            return "Remote request error: \(message)"
        case .parsingError(let message):
            return "Failed to parse response: \(message)"
        case .serverError(let code):
            return "Internal server error: \(code)"
        case .tlsError(let message):
            return "TLS Client error: \(message)"
        case .invalidRequest(let message):
            return "Invalid request: \(message)"
        case .unknown:
            return "Unknown error"
        }
    }
}
