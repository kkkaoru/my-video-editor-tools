// Runs in the page before the app: answers Tauri IPC from a fixture so every state can be shown without a backend.
(() => {
  const listeners = new Map();
  const callbacks = new Map();
  let nextCallback = 1;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    transformCallback(callback) {
      const id = nextCallback++;
      callbacks.set(id, callback);
      return id;
    },
    unregisterCallback(id) {
      callbacks.delete(id);
    },
    invoke(command, args) {
      if (command === "plugin:event|listen") {
        listeners.set(args.event, [...(listeners.get(args.event) ?? []), args.handler]);
        return Promise.resolve(args.handler);
      }
      if (command === "snapshot") {
        return Promise.resolve(window.__FIXTURE_SNAPSHOT__);
      }
      if (command === "save_settings") {
        return Promise.resolve({ ...window.__FIXTURE_SNAPSHOT__, settings: args.settings });
      }
      return Promise.resolve(null);
    },
  };
  window.__emitTauri = (event, payload) => {
    (listeners.get(event) ?? []).forEach((id) => {
      callbacks.get(id)?.({ event, id: 0, payload });
    });
  };
})();
