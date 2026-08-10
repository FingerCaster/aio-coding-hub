import { AIO_REPO_URL } from "../../constants/urls";
import {
  desktopUpdaterCheck,
  desktopUpdaterDiscard,
  desktopUpdaterDownloadAndInstall,
  parseDesktopUpdaterCheck,
  type DesktopUpdaterCheck,
  type DesktopUpdaterDownloadEvent,
} from "../desktop/updater";
import { normalizeUpdateChannel, type UpdateChannel } from "./updateChannel";

export type UpdaterCheckUpdate = DesktopUpdaterCheck & {
  generation?: number;
};

export type UpdaterCheckResult = UpdaterCheckUpdate | null;

const GITHUB_RELEASE_FALLBACK_RE = /^See release:\s+(\S+)$/;
const GITHUB_RELEASE_BODY_TIMEOUT_MS = 5_000;
const AIO_REPO_PATH = new URL(AIO_REPO_URL).pathname.split("/").filter(Boolean);

export function parseUpdaterCheckResult(
  value: unknown,
  expectedChannel: UpdateChannel = "stable"
): UpdaterCheckResult {
  return parseDesktopUpdaterCheck(value, expectedChannel);
}

export function updaterCandidateChannel(update: UpdaterCheckResult): UpdateChannel | null {
  return update?.channel === "stable" || update?.channel === "beta" ? update.channel : null;
}

/** A Beta subscription can legitimately select a later stable Release. */
export function updaterCandidateIsBetaPrerelease(update: UpdaterCheckResult): boolean {
  return updaterCandidateChannel(update) === "beta" && update?.isPrerelease === true;
}

export function isExactAioReleaseUrl(value: unknown): value is string {
  if (typeof value !== "string") return false;

  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return false;
  }

  const [owner, repo, releases, tagMarker, tag, ...rest] = url.pathname.split("/").filter(Boolean);
  return (
    url.protocol === "https:" &&
    url.host === "github.com" &&
    url.username.length === 0 &&
    url.password.length === 0 &&
    url.search.length === 0 &&
    url.hash.length === 0 &&
    owner === AIO_REPO_PATH[0] &&
    repo === AIO_REPO_PATH[1] &&
    releases === "releases" &&
    tagMarker === "tag" &&
    Boolean(tag) &&
    rest.length === 0
  );
}

function parseGitHubReleaseFallbackBody(body?: string | null) {
  const match = typeof body === "string" ? GITHUB_RELEASE_FALLBACK_RE.exec(body.trim()) : null;
  if (!match) return null;

  let url: URL;
  try {
    url = new URL(match[1]);
  } catch {
    return null;
  }

  const [owner, repo, releases, tagMarker, encodedTag, ...rest] = url.pathname
    .split("/")
    .filter(Boolean);
  if (
    url.protocol !== "https:" ||
    url.hostname !== "github.com" ||
    owner !== AIO_REPO_PATH[0] ||
    repo !== AIO_REPO_PATH[1] ||
    releases !== "releases" ||
    tagMarker !== "tag" ||
    !encodedTag ||
    rest.length > 0
  ) {
    return null;
  }

  return {
    owner,
    repo,
    tag: decodeURIComponent(encodedTag),
  };
}

function createReleaseBodyAbortSignal(): AbortSignal | undefined {
  if (typeof AbortSignal === "undefined" || typeof AbortSignal.timeout !== "function") {
    return undefined;
  }
  return AbortSignal.timeout(GITHUB_RELEASE_BODY_TIMEOUT_MS);
}

async function fetchGitHubReleaseBody(release: {
  owner: string;
  repo: string;
  tag: string;
}): Promise<string | null> {
  const response = await fetch(
    `https://api.github.com/repos/${release.owner}/${release.repo}/releases/tags/${encodeURIComponent(release.tag)}`,
    {
      headers: { accept: "application/vnd.github+json" },
      signal: createReleaseBodyAbortSignal(),
    }
  );
  if (!response.ok) return null;

  const value = (await response.json()) as { body?: unknown };
  if (typeof value.body !== "string" || value.body.trim().length === 0) {
    return null;
  }

  return value.body;
}

async function resolveGitHubReleaseFallbackBody(
  update: UpdaterCheckResult
): Promise<UpdaterCheckResult> {
  const release = parseGitHubReleaseFallbackBody(update?.body);
  if (!update || !release) return update;

  try {
    const body = await fetchGitHubReleaseBody(release);
    return body ? { ...update, body } : update;
  } catch {
    return update;
  }
}

export async function updaterCheck(channel: UpdateChannel = "stable"): Promise<UpdaterCheckResult> {
  const expectedChannel = normalizeUpdateChannel(channel);
  return resolveGitHubReleaseFallbackBody(await desktopUpdaterCheck({ channel: expectedChannel }));
}

export async function updaterDiscard(rid: number): Promise<boolean> {
  return desktopUpdaterDiscard(rid);
}

export async function updaterDownloadAndInstall(options: {
  rid: number;
  onEvent?: (event: DesktopUpdaterDownloadEvent) => void;
  timeoutMs?: number;
}): Promise<boolean | null> {
  return desktopUpdaterDownloadAndInstall(options);
}

export type UpdaterDownloadEvent = DesktopUpdaterDownloadEvent;
export { settingsUpdateChannelSet } from "./updateChannel";
export type { UpdateChannel } from "./updateChannel";
