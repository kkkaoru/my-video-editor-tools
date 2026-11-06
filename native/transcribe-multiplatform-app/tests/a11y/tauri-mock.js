// Runs in the page before the app: answers Tauri IPC from a fixture so every state can be shown without a backend.
(() => {
  const listeners = new Map();
  const callbacks = new Map();
  let nextCallback = 1;
  // As many families as a designer's computer can have: the font list has to stay fast with this.
  window.__FIXTURE_FONTS__ = [
    "Noto Sans JP",
    "Hiragino Sans",
    ...Array.from({ length: 3000 }, (_, index) => `Family ${String(index).padStart(4, "0")}`),
  ];
  const answerUpdate = (command) => {
    const { available, checkError, installError } = window.__FIXTURE_UPDATER__ ?? {};
    if (command === "check_update") {
      return checkError ? Promise.reject(checkError) : Promise.resolve(available ?? null);
    }
    if (installError) {
      return Promise.reject(installError);
    }
    window.__emitTauri("update-progress", 0.4);
    // A real install restarts the app, so it never answers.
    return new Promise(() => {});
  };
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
      if (command === "check_update" || command === "install_update") {
        return answerUpdate(command);
      }
      if (command === "font_families") {
        return Promise.resolve(window.__FIXTURE_FONTS__);
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
