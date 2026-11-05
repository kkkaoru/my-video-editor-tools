// Waits until nobody has touched the keyboard or mouse for the given seconds, so GUI checks never collide with the user.
import CoreGraphics
import Foundation
let needed = Double(CommandLine.arguments[1])!
let limit = Double(CommandLine.arguments[2])!
let start = Date()
while Date().timeIntervalSince(start) < limit {
  let idle = CGEventSource.secondsSinceLastEventType(.combinedSessionState, eventType: CGEventType(rawValue: ~0)!)
  if idle >= needed { print("idle", Int(idle)); exit(0) }
  sleep(2)
}
print("busy"); exit(1)
