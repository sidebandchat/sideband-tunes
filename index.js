const { EventEmitter } = require("node:events");
const native = require("./native.js");

class MediaSessionObserver extends EventEmitter {
  #native = new native.NativeMediaSessionObserver();
  #sessions = new Map();
  #started = false;

  get sessions() {
    return [...this.#sessions.values()];
  }

  async start() {
    if (this.#started) return this.sessions;
    this.#started = true;
    try {
      for (const session of await native.getSessions()) {
        this.#sessions.set(session.id, session);
      }
      await this.#native.start((event) => this.#handle(event));
      this.emit("reset", this.sessions);
      return this.sessions;
    } catch (error) {
      this.#started = false;
      throw error;
    }
  }

  async refresh() {
    const sessions = await native.getSessions();
    this.#sessions = new Map(sessions.map((session) => [session.id, session]));
    this.emit("reset", this.sessions);
    return this.sessions;
  }

  async close() {
    if (!this.#started) return;
    this.#started = false;
    await this.#native.stop();
    this.#sessions.clear();
  }

  #handle(event) {
    if (event.kind === "added" || event.kind === "changed") {
      if (!event.session) return;
      this.#sessions.set(event.session.id, event.session);
      this.emit(event.kind, event.session);
      this.emit("change", event);
      return;
    }
    if (event.kind === "removed") {
      if (!event.sessionId) return;
      this.#sessions.delete(event.sessionId);
      this.emit("removed", event.sessionId);
      this.emit("change", event);
      return;
    }
    if (event.kind === "reset") {
      void this.refresh().catch((error) => this.emit("error", error));
      return;
    }
    if (event.kind === "error") {
      this.emit(
        "error",
        new Error(event.message || "Media session backend failed"),
      );
    }
  }
}

module.exports = {
  MediaSessionObserver,
  getSessions: native.getSessions,
  getPlatformCapabilities: native.getPlatformCapabilities,
};
