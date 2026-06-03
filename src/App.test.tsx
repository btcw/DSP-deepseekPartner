import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { api, copyText } from "./api";
import { GatewayProfile, ProfileStatus } from "./types";

vi.mock("./api", () => ({
  api: {
    listProfiles: vi.fn(),
    saveProfile: vi.fn(),
    deleteProfile: vi.fn(),
    startProfile: vi.fn(),
    stopProfile: vi.fn(),
    startAll: vi.fn(),
    stopAll: vi.fn(),
    statuses: vi.fn(),
    readLogs: vi.fn(),
    clearLogs: vi.fn(),
    copyProxyText: vi.fn()
  },
  copyText: vi.fn()
}));

const profile: GatewayProfile = {
  id: "p1",
  name: "DeepSeek Local",
  port: 17777,
  upstreamBaseUrl: "https://api.deepseek.com",
  enabledSurfaces: ["anthropic", "openAi"],
  modelMapping: {
    main: "deepseek-v4-pro[1m]",
    opus: "deepseek-v4-pro[1m]",
    sonnet: "deepseek-v4-pro[1m]",
    haiku: "deepseek-v4-flash",
    subagent: "deepseek-v4-flash"
  },
  timeoutSeconds: 120,
  logLevel: "info",
  features: {
    normalizeAdaptiveThinking: true,
    mapEffort: true,
    preserveSse: true,
    smoothStreamingText: true,
    reasoningReplay: true,
    oneMContextDefaults: true,
    redactSensitiveLogs: true
  }
};

const stopped: ProfileStatus = {
  id: "p1",
  status: "stopped",
  port: 17777,
  proxyOrigin: "http://127.0.0.1:17777",
  requestCount: 0
};

const running: ProfileStatus = {
  ...stopped,
  status: "running",
  requestCount: 3,
  startedAt: "2026-06-03T12:00:00Z"
};

describe("App", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.listProfiles).mockResolvedValue([profile]);
    vi.mocked(api.statuses).mockResolvedValue([stopped]);
    vi.mocked(api.readLogs).mockResolvedValue([]);
  });

  it("saves a new profile from the add dialog", async () => {
    vi.mocked(api.listProfiles).mockResolvedValue([]);
    vi.mocked(api.statuses).mockResolvedValue([]);
    vi.mocked(api.saveProfile).mockResolvedValue([{ ...profile, id: "new" }]);
    const user = userEvent.setup();
    render(<App />);

    await screen.findByText("No profiles");
    await user.click(screen.getByRole("button", { name: /add profile/i }));
    await user.clear(screen.getByLabelText("Name"));
    await user.type(screen.getByLabelText("Name"), "Claude Code Proxy");
    await user.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() => expect(api.saveProfile).toHaveBeenCalled());
    expect(vi.mocked(api.saveProfile).mock.calls[0][0]).toMatchObject({
      name: "Claude Code Proxy",
      port: 17777,
      upstreamBaseUrl: "https://api.deepseek.com"
    });
  });

  it("starts and stops a profile", async () => {
    vi.mocked(api.startProfile).mockResolvedValue(running);
    vi.mocked(api.stopProfile).mockResolvedValue(stopped);
    const user = userEvent.setup();
    render(<App />);

    await screen.findByText("DeepSeek Local");
    await user.click(screen.getByRole("button", { name: /^start$/i }));
    await waitFor(() => expect(screen.getByText("running")).toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: /^stop$/i }));
    await waitFor(() => expect(api.stopProfile).toHaveBeenCalledWith("p1"));
  });

  it("copies generated proxy text", async () => {
    vi.mocked(api.copyProxyText).mockResolvedValue("http://127.0.0.1:17777/anthropic");
    const user = userEvent.setup();
    render(<App />);

    await screen.findByText("DeepSeek Local");
    await user.click(screen.getByRole("button", { name: /anthropic url/i }));

    await waitFor(() => expect(api.copyProxyText).toHaveBeenCalledWith("p1", "anthropicUrl"));
    expect(copyText).toHaveBeenCalledWith("http://127.0.0.1:17777/anthropic");
  });

  it("opens logs and renders entries", async () => {
    vi.mocked(api.readLogs).mockResolvedValue([
      {
        timestamp: "2026-06-03T12:00:00Z",
        level: "info",
        profileId: "p1",
        requestId: "req-1",
        message: "POST /v1/messages -> https://api.deepseek.com/anthropic/v1/messages"
      }
    ]);
    const user = userEvent.setup();
    render(<App />);

    await screen.findByText("DeepSeek Local");
    await user.click(screen.getByRole("button", { name: /^logs$/i }));

    const panel = await screen.findByRole("complementary");
    expect(within(panel).getByText("req-1")).toBeInTheDocument();
    expect(within(panel).getByText(/api.deepseek.com/)).toBeInTheDocument();
  });
});
