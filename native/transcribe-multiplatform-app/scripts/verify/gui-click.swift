// Developer helper: clicks one point of the screen with real mouse events. Resolve's own lists ignore accessibility clicks.
import CoreGraphics
import Foundation

let point = CGPoint(x: Double(CommandLine.arguments[1])!, y: Double(CommandLine.arguments[2])!)
for type in [CGEventType.mouseMoved, .leftMouseDown, .leftMouseUp] {
  CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: .left)!.post(tap: .cghidEventTap)
  usleep(40000)
}
