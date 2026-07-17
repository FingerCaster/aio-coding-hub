import { afterEach, describe, expect, it, vi } from "vitest";
import type { ImageGenTaskRow } from "../../../services/image-gen/service";
import { imageGenHydrateImages, imageGenReadImage } from "../../../services/image-gen/service";
import {
  base64ToBlob,
  blobToBase64,
  buildPersistPayload,
  generateThumbnailB64,
  loadTaskAssets,
  mergeTasksByCreatedAt,
  parseRequestSnapshot,
  pruneTasksForCleanup,
  readBackReferenceImages,
  stripRequestSnapshot,
  taskFromRow,
  tasksFromRows,
  taskImageSrc,
  taskImageThumbSrc,
} from "../imageGenPersistence";
import { makeDiskImage, makeMemoryImage, makeTask, TEST_REQUEST } from "./testUtils";

vi.mock("../../../services/image-gen/service", async () => {
  const actual = await vi.importActual<typeof import("../../../services/image-gen/service")>(
    "../../../services/image-gen/service"
  );
  return { ...actual, imageGenHydrateImages: vi.fn(), imageGenReadImage: vi.fn() };
});

function makeRow(overrides: Partial<ImageGenTaskRow> = {}): ImageGenTaskRow {
  return {
    id: "row-1",
    adapterId: "gpt-image",
    prompt: "历史任务",
    requestJson: stripRequestSnapshot(TEST_REQUEST),
    status: "done",
    error: null,
    usageJson: null,
    images: [
      {
        path: "row-1/image-1.png",
        thumbPath: "row-1/thumb-1.webp",
        mime: "image/png",
      },
    ],
    refImages: [],
    dir: "row-1",
    createdAt: 1_700_000_000_000,
    elapsedMs: 1200,
    ...overrides,
  };
}

describe("pages/image-gen/imageGenPersistence", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("blobToBase64/base64ToBlob roundtrip preserves bytes and mime", async () => {
    const blob = base64ToBlob(btoa("hello"), "image/png");
    expect(blob.type).toBe("image/png");
    await expect(blobToBase64(blob)).resolves.toBe(btoa("hello"));
  });

  it("taskImageSrc/taskImageThumbSrc pick the right url per form", () => {
    const memory = makeMemoryImage("blob:m");
    expect(taskImageSrc(memory)).toBe("blob:m");
    expect(taskImageThumbSrc(memory)).toBe("blob:m");

    const disk = makeDiskImage("/store/t1/image-1.png");
    expect(taskImageSrc(disk)).toBe("asset://localhost//store/t1/image-1.png");
    expect(taskImageThumbSrc(disk)).toBe("asset://localhost//store/t1/thumb-1.png");
  });

  it("generateThumbnailB64 downscales to 384px webp via canvas", async () => {
    const close = vi.fn();
    vi.stubGlobal(
      "createImageBitmap",
      vi.fn(async () => ({ width: 768, height: 512, close }))
    );
    const drawImage = vi.fn();
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
      drawImage,
    } as unknown as CanvasRenderingContext2D);
    vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation(function (
      this: HTMLCanvasElement,
      callback: BlobCallback
    ) {
      callback(new Blob(["thumb"], { type: "image/webp" }));
    });

    const thumb = await generateThumbnailB64(new Blob(["img"], { type: "image/png" }));
    expect(thumb).toEqual({ mime: "image/webp", dataB64: btoa("thumb") });
    // 长边 768 → 384，等比缩放。
    expect(drawImage).toHaveBeenCalledWith(expect.anything(), 0, 0, 384, 256);
    expect(close).toHaveBeenCalled();
  });

  it("generateThumbnailB64 returns null when the environment lacks canvas pieces", async () => {
    const blob = new Blob(["img"], { type: "image/png" });
    // jsdom 默认无 createImageBitmap。
    await expect(generateThumbnailB64(blob)).resolves.toBeNull();

    // 有 bitmap 但 canvas 2d 不可用。
    vi.stubGlobal(
      "createImageBitmap",
      vi.fn(async () => ({ width: 10, height: 10, close: vi.fn() }))
    );
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    await expect(generateThumbnailB64(blob)).resolves.toBeNull();

    // toBlob 产出 null。
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
      drawImage: vi.fn(),
    } as unknown as CanvasRenderingContext2D);
    vi.spyOn(HTMLCanvasElement.prototype, "toBlob").mockImplementation(function (
      this: HTMLCanvasElement,
      callback: BlobCallback
    ) {
      callback(null);
    });
    await expect(generateThumbnailB64(blob)).resolves.toBeNull();
  });

  it("stripRequestSnapshot replaces reference bytes with file placeholders", () => {
    const json = stripRequestSnapshot({
      ...TEST_REQUEST,
      referenceImages: [
        { mime: "image/png", b64: btoa("secret-bytes") },
        { mime: "image/webp", b64: btoa("more-bytes") },
      ],
    });
    expect(json).not.toContain(btoa("secret-bytes"));
    const parsed = JSON.parse(json) as { referenceImages: unknown };
    expect(parsed.referenceImages).toEqual([
      { file: "ref-1.png", mime: "image/png" },
      { file: "ref-2.webp", mime: "image/webp" },
    ]);
  });

  it("parseRequestSnapshot restores placeholders as empty-b64 refs and rejects bad shapes", () => {
    const restored = parseRequestSnapshot(
      stripRequestSnapshot({
        ...TEST_REQUEST,
        referenceImages: [{ mime: "image/png", b64: btoa("x") }],
      })
    );
    expect(restored.prompt).toBe(TEST_REQUEST.prompt);
    expect(restored.options).toEqual(TEST_REQUEST.options);
    expect(restored.referenceImages).toEqual([{ mime: "image/png", b64: "" }]);

    expect(() => parseRequestSnapshot("not-json{")).toThrow();
    expect(() => parseRequestSnapshot('"just-a-string"')).toThrow();
    expect(() => parseRequestSnapshot('{"prompt":"x"}')).toThrow(); // 缺 options
  });

  it("parseRequestSnapshot tolerates malformed reference entries with a png fallback", () => {
    const restored = parseRequestSnapshot(
      JSON.stringify({ ...TEST_REQUEST, referenceImages: [{ file: "ref-1.bin" }, null] })
    );
    expect(restored.referenceImages).toEqual([
      { mime: "image/png", b64: "" },
      { mime: "image/png", b64: "" },
    ]);
  });

  it("buildPersistPayload maps a done memory task with refs (thumbs default empty in jsdom)", async () => {
    const task = makeTask({
      id: "t-persist",
      status: "done",
      images: [makeMemoryImage()],
      usage: { totalTokens: 7 },
      request: {
        ...TEST_REQUEST,
        referenceImages: [{ mime: "image/png", b64: btoa("ref") }],
      },
      elapsedMs: 900,
    });
    const payload = await buildPersistPayload(task);
    expect(payload).toMatchObject({
      id: "t-persist",
      adapterId: "gpt-image",
      prompt: "一只猫",
      status: "done",
      error: null,
      usageJson: JSON.stringify({ totalTokens: 7 }),
      createdAt: task.createdAt,
      elapsedMs: 900,
      thumbs: [],
      refImages: [{ mime: "image/png", dataB64: btoa("ref") }],
    });
    expect(payload.images).toHaveLength(1);
    expect(payload.images[0].mime).toBe("image/png");
    expect(payload.images[0].dataB64.length).toBeGreaterThan(0);
  });

  it("buildPersistPayload maps error tasks and skips empty-b64 refs and disk images", async () => {
    const task = makeTask({
      status: "error",
      error: "HTTP 500: boom",
      images: [makeDiskImage()],
      request: { ...TEST_REQUEST, referenceImages: [{ mime: "image/png", b64: "" }] },
      elapsedMs: undefined,
    });
    const payload = await buildPersistPayload(task);
    expect(payload.status).toBe("error");
    expect(payload.error).toBe("HTTP 500: boom");
    expect(payload.usageJson).toBeNull();
    expect(payload.elapsedMs).toBeNull();
    // disk 图与空 b64 参考图都不重传。
    expect(payload.images).toEqual([]);
    expect(payload.refImages).toEqual([]);
  });

  it("taskFromRow hydrates thumbnails without reading originals or references", async () => {
    vi.mocked(imageGenReadImage).mockImplementation(async (reference) => ({
      mime: reference.endsWith(".webp") ? "image/webp" : "image/png",
      dataB64: btoa(reference),
    }));
    const row = makeRow({
      usageJson: JSON.stringify({ totalTokens: 5 }),
      refImages: [{ path: "row-1/ref-1.png", thumbPath: null, mime: "image/png" }],
    });
    const task = await taskFromRow(row);
    expect(task).not.toBeNull();
    expect(task).toMatchObject({
      id: "row-1",
      prompt: "历史任务",
      status: "done",
      persisted: true,
      createdAt: row.createdAt,
      startedAt: row.createdAt,
      elapsedMs: 1200,
      usage: { totalTokens: 5 },
      refThumbs: [],
      refPaths: [{ path: "row-1/ref-1.png", mime: "image/png" }],
    });
    expect(task?.images[0]).toEqual({
      kind: "disk",
      src: null,
      thumbSrc: `data:image/webp;base64,${btoa("row-1/thumb-1.webp")}`,
      path: "row-1/image-1.png",
      mime: "image/png",
    });
    expect(imageGenReadImage).toHaveBeenCalledTimes(1);
    expect(imageGenReadImage).toHaveBeenCalledWith("row-1/thumb-1.webp");
  });

  it("taskFromRow leaves a missing thumbnail lazy and tolerates bad usage", async () => {
    vi.mocked(imageGenReadImage).mockResolvedValue({ mime: "image/png", dataB64: btoa("image") });
    const row = makeRow({
      status: "error",
      error: "boom",
      usageJson: "not-json{",
      elapsedMs: null,
      images: [{ path: "row-1/image-1.png", thumbPath: null, mime: "image/png" }],
    });
    const task = await taskFromRow(row);
    expect(task?.status).toBe("error");
    expect(task?.error).toBe("boom");
    expect(task?.usage).toBeUndefined();
    expect(task?.elapsedMs).toBeUndefined();
    expect(taskImageThumbSrc(task!.images[0])).toBe("");
    expect(imageGenReadImage).not.toHaveBeenCalled();
  });

  it("loads original and reference images only on demand", async () => {
    vi.mocked(imageGenReadImage).mockImplementation(async (reference) => ({
      mime: "image/png",
      dataB64: btoa(reference),
    }));
    const task = await taskFromRow(
      makeRow({ refImages: [{ path: "row-1/ref-1.png", thumbPath: null, mime: "image/png" }] })
    );
    expect(task).not.toBeNull();
    vi.mocked(imageGenReadImage).mockClear();

    const loaded = await loadTaskAssets(task!);

    expect(imageGenReadImage).toHaveBeenCalledWith("row-1/image-1.png");
    expect(imageGenReadImage).toHaveBeenCalledWith("row-1/ref-1.png");
    expect(taskImageSrc(loaded.images[0])).toContain(btoa("row-1/image-1.png"));
    expect(loaded.refThumbs[0]).toContain(btoa("row-1/ref-1.png"));
  });

  it("hydrates the first page through one backend-budgeted production command", async () => {
    vi.mocked(imageGenHydrateImages).mockImplementation(async (paths) =>
      paths.map((path) => ({ mime: "image/webp", dataB64: btoa(path) }))
    );
    const rows = Array.from({ length: 12 }, (_, index) => makeRow({ id: `row-${index}` }));

    await tasksFromRows(rows);

    expect(imageGenHydrateImages).toHaveBeenCalledTimes(1);
    expect(imageGenHydrateImages).toHaveBeenCalledWith(rows.map((row) => row.images[0].thumbPath));
    expect(imageGenReadImage).not.toHaveBeenCalled();
  });

  it("taskFromRow returns null for unparsable request snapshots", async () => {
    const warnSpy = vi.spyOn(console, "warn").mockImplementation(() => {});
    await expect(taskFromRow(makeRow({ requestJson: "not-json{" }))).resolves.toBeNull();
    expect(warnSpy).toHaveBeenCalled();
    expect(JSON.stringify(warnSpy.mock.calls)).not.toContain("not-json");
  });

  it("mergeTasksByCreatedAt dedupes by id (store wins) and sorts ascending", () => {
    const current = [
      makeTask({ id: "b", createdAt: 200, prompt: "store 版本" }),
      makeTask({ id: "d", createdAt: 400 }),
    ];
    const incoming = [
      makeTask({ id: "b", createdAt: 200, prompt: "DB 版本" }),
      makeTask({ id: "a", createdAt: 100 }),
      makeTask({ id: "c", createdAt: 300 }),
    ];
    const merged = mergeTasksByCreatedAt(current, incoming);
    expect(merged.map((task) => task.id)).toEqual(["a", "b", "c", "d"]);
    expect(merged[1].prompt).toBe("store 版本");
  });

  it("pruneTasksForCleanup keeps the newest N persisted tasks and all memory tasks", () => {
    const tasks = [
      makeTask({ id: "old", createdAt: 100, persisted: true }),
      makeTask({ id: "mid", createdAt: 200, persisted: true }),
      makeTask({ id: "memory", createdAt: 150, persisted: false }),
      makeTask({ id: "new", createdAt: 300, persisted: true }),
    ];
    expect(pruneTasksForCleanup(tasks, 2).map((task) => task.id)).toEqual(["mid", "memory", "new"]);
    // keepCount 0：persisted 全清，memory 保留。
    expect(pruneTasksForCleanup(tasks, 0).map((task) => task.id)).toEqual(["memory"]);
    // keepCount 超过总数：不变。
    expect(pruneTasksForCleanup(tasks, 10)).toEqual(tasks);
  });

  it("readBackReferenceImages reads each path and propagates failures", async () => {
    vi.mocked(imageGenReadImage)
      .mockResolvedValueOnce({ mime: "image/png", dataB64: btoa("one") })
      .mockResolvedValueOnce({ mime: "image/webp", dataB64: btoa("two") });
    await expect(
      readBackReferenceImages([
        { path: "/store/t/ref-1.png", mime: "image/png" },
        { path: "/store/t/ref-2.webp", mime: "image/webp" },
      ])
    ).resolves.toEqual([
      { mime: "image/png", b64: btoa("one") },
      { mime: "image/webp", b64: btoa("two") },
    ]);
    expect(imageGenReadImage).toHaveBeenCalledWith("/store/t/ref-1.png");
    expect(imageGenReadImage).toHaveBeenCalledWith("/store/t/ref-2.webp");

    vi.mocked(imageGenReadImage).mockRejectedValueOnce(new Error("missing"));
    await expect(
      readBackReferenceImages([{ path: "/store/t/ref-1.png", mime: "image/png" }])
    ).rejects.toThrow("missing");
  });
});
