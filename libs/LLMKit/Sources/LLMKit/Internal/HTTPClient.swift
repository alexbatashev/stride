import Foundation
#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

enum HTTPClient {
    static func request(
        method: String,
        url: URL,
        headers: [String: String] = [:],
        body: Data? = nil
    ) async throws -> (HTTPURLResponse, Data) {
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.httpBody = body

        for (key, value) in headers {
            request.setValue(value, forHTTPHeaderField: key)
        }

        do {
            let (data, response) = try await URLSession.shared.data(for: request)
            guard let httpResponse = response as? HTTPURLResponse else {
                throw LLMError.unknown
            }
            return (httpResponse, data)
        } catch {
            throw LLMError.requestError(String(describing: error))
        }
    }

    static func streamLines(
        method: String,
        url: URL,
        headers: [String: String] = [:],
        body: Data? = nil
    ) -> AsyncThrowingStream<String, Error> {
        AsyncThrowingStream { continuation in
            Task {
                do {
                    var request = URLRequest(url: url)
                    request.httpMethod = method
                    request.httpBody = body

                    for (key, value) in headers {
                        request.setValue(value, forHTTPHeaderField: key)
                    }

                    let (bytes, response) = try await URLSession.shared.bytes(for: request)
                    guard let httpResponse = response as? HTTPURLResponse else {
                        throw LLMError.unknown
                    }

                    guard (200..<300).contains(httpResponse.statusCode) else {
                        throw LLMError.serverError(httpResponse.statusCode)
                    }

                    for try await line in bytes.lines {
                        continuation.yield(line)
                    }
                    continuation.finish()
                } catch {
                    if let err = error as? LLMError {
                        continuation.finish(throwing: err)
                    } else {
                        continuation.finish(throwing: LLMError.requestError(String(describing: error)))
                    }
                }
            }
        }
    }
}
