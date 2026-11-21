import { describe, expect, it, vi } from "vitest";
import { createFakeBridge } from "../test/fakeBridge";
import { EVENT_LEVEL, EVENT_MIC_TEST_LEVEL } from "./contract";
import { listenShared } from "./shared";

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("listenShared", () => {
  it("fans one native listener out to every follower", async () => {
    const bridge = createFakeBridge();
    const first = vi.fn();
    const second = vi.fn();
    const stopFirst = listenShared(bridge, EVENT_LEVEL, first);
    const stopSecond = listenShared(bridge, EVENT_LEVEL, second);
    await settle();
    expect(bridge.listen).toHaveBeenCalledTimes(1);
    expect(bridge.listenerCount(EVENT_LEVEL)).toBe(1);

    bridge.emit(EVENT_LEVEL, 0.4);
    expect(first).toHaveBeenCalledWith(0.4);
    expect(second).toHaveBeenCalledWith(0.4);

    stopFirst();
    stopFirst();
    bridge.emit(EVENT_LEVEL, 0.6);
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(2);

    stopSecond();
    await settle();
    expect(bridge.listenerCount(EVENT_LEVEL)).toBe(0);

    // A new follower opens a new native listener.
    const third = vi.fn();
    const stopThird = listenShared(bridge, EVENT_LEVEL, third);
    await settle();
    bridge.emit(EVENT_LEVEL, 0.2);
    expect(third).toHaveBeenCalledWith(0.2);
    stopThird();
  });

  it("keeps events apart", async () => {
    const bridge = createFakeBridge();
    const level = vi.fn();
    const micTest = vi.fn();
    const stops = [listenShared(bridge, EVENT_LEVEL, level), listenShared(bridge, EVENT_MIC_TEST_LEVEL, micTest)];
    await settle();
    bridge.emit(EVENT_MIC_TEST_LEVEL, 0.3);
    expect(level).not.toHaveBeenCalled();
    expect(micTest).toHaveBeenCalledWith(0.3);
    stops.forEach((stop) => stop());
  });
});
