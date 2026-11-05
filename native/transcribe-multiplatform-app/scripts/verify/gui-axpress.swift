// Presses a button by title through accessibility: no mouse, no keyboard, no focus change.
import ApplicationServices
import Foundation
let pid = Int32(CommandLine.arguments[1])!, wanted = CommandLine.arguments[2]
func attr(_ e: AXUIElement, _ n: String) -> AnyObject? { var v: AnyObject?; AXUIElementCopyAttributeValue(e, n as CFString, &v); return v }
func find(_ e: AXUIElement, _ d: Int) -> AXUIElement? {
  if (attr(e, "AXRole") as? String) == "AXButton", (attr(e, "AXTitle") as? String) == wanted { return e }
  if d > 6 { return nil }
  for c in attr(e, "AXChildren") as? [AXUIElement] ?? [] { if let f = find(c, d + 1) { return f } }
  return nil
}
for w in attr(AXUIElementCreateApplication(pid), "AXWindows") as? [AXUIElement] ?? [] {
  if let b = find(w, 0) { print("press", wanted, AXUIElementPerformAction(b, "AXPress" as CFString).rawValue); exit(0) }
}
print("not found", wanted); exit(1)
