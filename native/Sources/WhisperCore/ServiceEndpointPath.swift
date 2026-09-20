import Foundation

/// Match the existing gateway suffix normalization without changing arbitrary path casing.
func normalizedServiceBasePath(_ input: String) -> String {
    var path = input
    while path.hasSuffix("/") { path.removeLast() }
    for suffix in ["/audio/transcriptions", "/audio/translations", "/chat/completions", "/responses", "/models"] {
        if path.lowercased().hasSuffix(suffix) {
            path.removeLast(suffix.count)
            if path.lowercased().hasSuffix("/v1") { path.removeLast(3); path += "/v1" }
            break
        }
    }
    return path
}
