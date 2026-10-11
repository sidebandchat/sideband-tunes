import { EventEmitter } from "node:events";

export type PlaybackState = "playing" | "paused" | "stopped" | "unknown";
export type ChangeKind = "added" | "changed" | "removed" | "reset" | "error";
export type PlatformAvailability = "available" | "unsupported";

export interface PlatformValue {
  key: string;
  value: string;
}

export interface MediaSession {
  id: string;
  title?: string;
  artist?: string;
  album?: string;
  artworkUrl?: string;
  durationMs?: number;
  positionMs?: number;
  playbackState: PlaybackState;
  applicationId: string;
  applicationName?: string;
  trackId?: string;
  trackUri?: string;
  platform: string;
  platformValues: PlatformValue[];
}

export interface SessionChangeEvent {
  kind: ChangeKind;
  sessionId?: string;
  session?: MediaSession;
  message?: string;
}

export interface PlatformCapabilities {
  platform: string;
  supported: boolean;
  availability: PlatformAvailability;
  sessionEnumeration: boolean;
  eventNotifications: boolean;
  artwork: boolean;
  trackIds: boolean;
  trackUris: boolean;
  positionNotifications: boolean;
  notes: string[];
}

export function getSessions(): Promise<MediaSession[]>;
export function getPlatformCapabilities(): PlatformCapabilities;

export class MediaSessionObserver extends EventEmitter {
  readonly sessions: MediaSession[];
  start(): Promise<MediaSession[]>;
  refresh(): Promise<MediaSession[]>;
  close(): Promise<void>;
  on(
    event: "added" | "changed",
    listener: (session: MediaSession) => void,
  ): this;
  on(event: "removed", listener: (sessionId: string) => void): this;
  on(event: "reset", listener: (sessions: MediaSession[]) => void): this;
  on(event: "change", listener: (event: SessionChangeEvent) => void): this;
  on(event: "error", listener: (error: Error) => void): this;
}
