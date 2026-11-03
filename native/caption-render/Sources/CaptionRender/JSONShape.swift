import Foundation

/// The key structure of a JSON document, to refuse keys a contract does not have (the Python
/// readers of these files require exact key sets; `Codable` alone ignores extra keys).
indirect enum JSONShape: Decodable, Equatable {
  case object([String: JSONShape])
  case array([JSONShape])
  case value

  struct Key: CodingKey {
    let stringValue: String
    var intValue: Int? { nil }
    init(stringValue: String) { self.stringValue = stringValue }
    init?(intValue: Int) { nil }
  }

  init(from decoder: any Decoder) throws {
    do {
      let object = try decoder.container(keyedBy: Key.self)
      var members: [String: JSONShape] = [:]
      for key in object.allKeys {
        members[key.stringValue] = try object.decode(JSONShape.self, forKey: key)
      }
      self = .object(members)
      return
    } catch DecodingError.typeMismatch, DecodingError.valueNotFound {
      // Not an object (or null): try an array, then a scalar.
    }
    do {
      var array = try decoder.unkeyedContainer()
      var items: [JSONShape] = []
      while !array.isAtEnd { items.append(try array.decode(JSONShape.self)) }
      self = .array(items)
    } catch DecodingError.typeMismatch, DecodingError.valueNotFound {
      self = .value
    }
  }

  static func of(_ data: Data) throws -> JSONShape {
    do {
      return try JSONDecoder().decode(JSONShape.self, from: data)
    } catch {
      throw CaptionRenderError.invalid("Unreadable JSON: \(error)")
    }
  }

  /// The member `key` of an object (`.value` when absent or not an object).
  subscript(key: String) -> JSONShape {
    guard case .object(let members) = self, let member = members[key] else { return .value }
    return member
  }

  /// Whether this is an object with member `key` (even `null`).
  func has(_ key: String) -> Bool {
    guard case .object(let members) = self else { return false }
    return members[key] != nil
  }

  var items: [JSONShape] {
    guard case .array(let items) = self else { return [] }
    return items
  }

  /// Throws unless this is an object with every `required` key and no key outside `allowed`
  /// (non-objects, such as `null`, pass: their type is `Codable`'s to check).
  func requireKeys(_ required: Set<String>, allowed: Set<String>? = nil, in name: String) throws {
    guard case .object(let members) = self else { return }
    let keys = Set(members.keys)
    guard required.isSubset(of: keys), keys.isSubset(of: allowed ?? required) else {
      throw CaptionRenderError.invalid(
        "\(name) has keys \(keys.sorted()), not \((allowed ?? required).sorted())")
    }
  }
}
