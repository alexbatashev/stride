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
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.httpBody = body
        for (key, value) in headers {
            request.setValue(value, forHTTPHeaderField: key)
        }

        return AsyncThrowingStream { continuation in
            let delegate = LineStreamDelegate(continuation: continuation)
            let session = URLSession(configuration: .default, delegate: delegate, delegateQueue: nil)
            let task = session.dataTask(with: request)
            continuation.onTermination = { _ in
                task.cancel()
                session.invalidateAndCancel()
            }
            task.resume()
        }
    }
}

// MARK: - Delegate

/// Streams response body as newline-delimited strings, yielding each complete line as it arrives.
/// Works on both Apple platforms and Linux (FoundationNetworking).
private final class LineStreamDelegate: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    private let continuation: AsyncThrowingStream<String, Error>.Continuation
    private var statusCode = 0
    private var buffer = ""

    init(continuation: AsyncThrowingStream<String, Error>.Continuation) {
        self.continuation = continuation
    }

    func urlSession(
        _ session: URLSession,
        dataTask: URLSessionDataTask,
        didReceive response: URLResponse,
        completionHandler: @escaping (URLSession.ResponseDisposition) -> Void
    ) {
        if let http = response as? HTTPURLResponse {
            statusCode = http.statusCode
        }
        completionHandler(.allow)
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        guard (200..<300).contains(statusCode) else { return }
        buffer += String(decoding: data, as: UTF8.self)
        var lines = buffer.components(separatedBy: "\n")
        // The last element may be an incomplete line; keep it in the buffer.
        buffer = lines.removeLast()
        for line in lines {
            continuation.yield(line)
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        if let error {
            continuation.finish(throwing: LLMError.requestError(String(describing: error)))
        } else if !(200..<300).contains(statusCode) {
            continuation.finish(throwing: LLMError.serverError(statusCode))
        } else {
            if !buffer.isEmpty { continuation.yield(buffer) }
            continuation.finish()
        }
    }
}
