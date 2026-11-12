import { expect, test } from '@playwright/test';

const activeContexts = new Set();

async function newMeetingPage(browser, name, deniedDevice = '') {
  const context = await browser.newContext({
    permissions: ['camera', 'microphone', 'clipboard-read', 'clipboard-write'],
  });
  activeContexts.add(context);
  context.on('close', () => activeContexts.delete(context));
  if (deniedDevice) {
    await context.addInitScript((device) => {
      const original = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
      Object.defineProperty(navigator.mediaDevices, 'getUserMedia', { configurable: true, value: (constraints) => {
        if (constraints[device]) return Promise.reject(new DOMException(`${device} denied`, 'NotAllowedError'));
        return original(constraints);
      } });
    }, deniedDevice);
  }
  await context.addInitScript(() => {
    const NativePeerConnection = window.RTCPeerConnection;
    const NativeWebSocket = window.WebSocket;
    window.__peerConnections = [];
    window.__meetingSockets = [];
    Object.defineProperty(navigator.mediaDevices, 'enumerateDevices', {
      configurable: true,
      value: async () => [
        { deviceId: 'default-audio', kind: 'audioinput', label: 'Default microphone', groupId: 'default-group' },
        { deviceId: 'default-video', kind: 'videoinput', label: 'Default camera', groupId: 'default-group' },
      ],
    });
    Object.defineProperty(navigator.mediaDevices, 'getDisplayMedia', {
      configurable: true,
      value: async () => new MediaStream(),
    });
    window.RTCPeerConnection = function (...args) {
      const connection = new NativePeerConnection(...args);
      window.__peerConnections.push(connection);
      return connection;
    };
    const MeetingWebSocket = function (...args) {
      const socket = new NativeWebSocket(...args);
      window.__meetingSockets.push(socket);
      return socket;
    };
    Object.assign(MeetingWebSocket, {
      CONNECTING: NativeWebSocket.CONNECTING,
      OPEN: NativeWebSocket.OPEN,
      CLOSING: NativeWebSocket.CLOSING,
      CLOSED: NativeWebSocket.CLOSED,
    });
    window.WebSocket = MeetingWebSocket;
  });
  const page = await context.newPage();
  await page.goto('/');
  await page.getByLabel('Your name').fill(name);
  return { context, page };
}

test.afterEach(async () => {
  await Promise.all([...activeContexts].map((context) => context.close()));
});

async function createRoom(browser, name) {
  const participant = await newMeetingPage(browser, name);
  await participant.page.getByRole('button', { name: 'Create room' }).click();
  await expect(participant.page.locator('#meetingStatus')).toContainText('Connected. Your media is live.');
  return participant;
}

async function joinRoom(browser, name, room) {
  const participant = await newMeetingPage(browser, name);
  await participant.page.getByLabel('Room code or invite link').fill(room);
  await participant.page.getByRole('button', { name: 'Join room' }).click();
  await expect(participant.page.locator('#meetingStatus')).toContainText('Connected. Your media is live.');
  return participant;
}

async function peerMediaSnapshot(page) {
  return page.evaluate(() => {
    const connection = window.__peerConnections?.[0];
    return {
      signalingState: connection?.signalingState,
      senders: connection?.getSenders().map((sender) => sender.track?.id).filter(Boolean).sort(),
      receivers: connection?.getReceivers().map((receiver) => receiver.track?.kind).filter(Boolean).sort(),
      videos: [...document.querySelectorAll('.tile video')].map((video) => ({
        peerId: video.closest('.tile').dataset.peerId,
        local: video.getAttribute('aria-label') === 'Your camera preview',
        tracks: video.srcObject?.getTracks().map((track) => ({ id: track.id, kind: track.kind })).sort((a, b) => a.kind.localeCompare(b.kind)) || [],
      })),
    };
  });
}

async function inboundVideoBytes(page) {
  return page.evaluate(async () => {
    const connection = window.__peerConnections[0];
    const reports = await Promise.all(connection.getReceivers().map((receiver) => receiver.getStats()));
    return [...reports.flatMap((report) => [...report.values()])]
      .filter((stat) => stat.type === 'inbound-rtp' && stat.kind === 'video')
      .reduce((total, stat) => total + stat.bytesReceived, 0);
  });
}

test('three participants see stable media tiles and roster; controls preserve RTP and leave removes stale media', async ({ browser }) => {
  const host = await createRoom(browser, 'Host');
  const room = new URL(host.page.url()).searchParams.get('room');
  const guestA = await joinRoom(browser, 'Guest A', room);
  for (const page of [host.page, guestA.page]) {
    await expect.poll(async () => {
      const { signalingState, receivers } = await peerMediaSnapshot(page);
      return signalingState === 'stable' && receivers.length >= 2;
    }, { timeout: 30000 }).toBe(true);
  }
  const guestB = await joinRoom(browser, 'Guest B', room);

  for (const { page } of [host, guestA, guestB]) {
    await expect(page.locator('#rosterCount')).toHaveText('3 here');
    await expect(page.locator('.tile')).toHaveCount(3);
    const rosterNames = await page.locator('.participant-name').allTextContents();
    for (const name of ['Host', 'Guest A', 'Guest B']) {
      expect(rosterNames.some((rosterName) => rosterName.includes(name))).toBe(true);
    }
    await expect.poll(async () => {
      const { signalingState, receivers } = await peerMediaSnapshot(page);
      return signalingState === 'stable' && receivers.length >= 4;
    }, { timeout: 30000 }).toBe(true);
    const snapshot = await peerMediaSnapshot(page);
    expect(snapshot.receivers).toEqual(expect.arrayContaining(['audio', 'video']));
    expect(snapshot.videos.filter((video) => !video.local && video.tracks.length > 0)).toHaveLength(2);
  }

  const before = await peerMediaSnapshot(guestA.page);
  const inboundVideoBefore = await inboundVideoBytes(guestA.page);
  const gridButton = guestA.page.getByRole('button', { name: 'Grid' });
  await gridButton.focus();
  await guestA.page.keyboard.press('Enter');
  await expect(gridButton).toHaveAttribute('aria-pressed', 'true');
  const speakerButton = guestA.page.getByRole('button', { name: 'Speaker' });
  await speakerButton.focus();
  await guestA.page.keyboard.press('Enter');
  await expect(speakerButton).toHaveAttribute('aria-pressed', 'true');
  await expect(guestA.page.locator('.tile:visible')).toHaveCount(1);
  await guestA.page.getByRole('button', { name: 'Grid' }).click();

  const guestBPin = guestA.page.getByRole('button', { name: 'Pin Guest B' });
  await guestBPin.focus();
  await guestA.page.keyboard.press('Enter');
  await expect(guestA.page.getByRole('button', { name: 'Unpin Guest B' })).toHaveAttribute('aria-pressed', 'true');
  await guestA.page.getByRole('button', { name: 'Unpin Guest B' }).click();

  const mic = guestA.page.locator('#micToggle');
  await mic.focus();
  await guestA.page.keyboard.press('Enter');
  await expect(mic).toHaveAttribute('aria-pressed', 'false');
  await expect(host.page.locator('.participant').filter({ hasText: 'Guest A' })).toContainText('Mic off');
  const camera = guestA.page.locator('#cameraToggle');
  await camera.focus();
  await guestA.page.keyboard.press('Enter');
  await expect(camera).toHaveAttribute('aria-pressed', 'false');
  await expect(host.page.locator('.participant').filter({ hasText: 'Guest A' })).toContainText('Camera off');

  const copyInvite = guestA.page.getByRole('button', { name: 'Copy meeting invite' });
  await copyInvite.focus();
  await guestA.page.keyboard.press('Enter');
  const copiedInvite = await guestA.page.evaluate(() => navigator.clipboard.readText());
  expect(new URL(copiedInvite).searchParams.get('room')).toBe(room);
  expect(new URL(copiedInvite).searchParams.has('ticket')).toBe(false);

  const after = await peerMediaSnapshot(guestA.page);
  await expect.poll(() => inboundVideoBytes(guestA.page)).toBeGreaterThan(inboundVideoBefore);
  expect(after.signalingState).toBe(before.signalingState);
  expect(after.senders).toEqual(before.senders);
  for (const video of after.videos) {
    expect(video.tracks.map((track) => track.id)).toEqual(before.videos.find((initial) => initial.peerId === video.peerId).tracks.map((track) => track.id));
  }

  const hostLocalTrackIds = await host.page.locator('.tile video[aria-label="Your camera preview"]').evaluate((video) => video.srcObject.getTracks().map((track) => track.id).sort());
  await guestB.page.getByRole('button', { name: 'Leave' }).focus();
  await guestB.page.keyboard.press('Enter');
  await expect(host.page.locator('#rosterCount')).toHaveText('2 here');
  await expect(guestA.page.locator('#rosterCount')).toHaveText('2 here');
  await expect(host.page.locator('.tile')).toHaveCount(2);
  await expect(guestA.page.locator('.tile')).toHaveCount(2);
  expect((await host.page.locator('.tile-name').allTextContents()).some((name) => name.includes('Guest B'))).toBe(false);
  const localPreviewTracks = await host.page.locator('.tile video[aria-label="Your camera preview"]').evaluate((video) => video.srcObject.getTracks().map((track) => track.kind));
  expect(localPreviewTracks).toEqual(expect.arrayContaining(['audio', 'video']));
  const hostLocalTrackIdsAfterLeave = await host.page.locator('.tile video[aria-label="Your camera preview"]').evaluate((video) => video.srcObject.getTracks().map((track) => track.id).sort());
  expect(hostLocalTrackIdsAfterLeave).toEqual(hostLocalTrackIds);

  await guestA.page.getByRole('button', { name: 'Leave' }).click();
  await host.page.getByRole('button', { name: 'Leave' }).click();
  await Promise.all([host.context.close(), guestA.context.close(), guestB.context.close()]);
});

test('screen sharing and device selectors are available in the meeting controls', async ({ browser }) => {
  const participant = await createRoom(browser, 'Screen user');
  await expect(participant.page.getByRole('button', { name: /share screen/i })).toBeVisible();
  await expect(participant.page.locator('#microphoneDevice')).toBeVisible();
  await expect(participant.page.locator('#cameraDevice')).toBeVisible();
  await participant.page.locator('#microphoneDevice').selectOption('default-audio');
  await participant.page.locator('#cameraDevice').selectOption('default-video');
  await participant.page.getByRole('button', { name: /share screen/i }).click();
  await expect(participant.page.getByRole('button', { name: /stop sharing/i })).toBeVisible();
  await participant.page.getByRole('button', { name: 'Leave' }).click();
});

test('signaling disconnects reconnect with a fresh ticket and the media surface restores', async ({ browser }) => {
  const host = await createRoom(browser, 'Host reconnect');
  const room = new URL(host.page.url()).searchParams.get('room');
  const guest = await joinRoom(browser, 'Guest reconnect', room);

  await guest.page.evaluate(() => {
    const socket = window.__meetingSockets[window.__meetingSockets.length - 1];
    if (socket) socket.close();
  });

  await expect.poll(async () => guest.page.evaluate(() => window.__meetingSockets.length), { timeout: 20000 }).toBeGreaterThan(1);
  await expect.poll(async () => guest.page.locator('#meetingStatus').textContent(), { timeout: 20000 }).toContain('Connected');
  await expect.poll(async () => {
    const total = await guest.page.locator('#rosterCount').textContent();
    const match = total.match(/(\d+) here/);
    return Number(match?.[1] ?? 0);
  }, { timeout: 20000 }).toBeGreaterThanOrEqual(2);

  await guest.page.getByRole('button', { name: 'Leave' }).click();
  await host.page.getByRole('button', { name: 'Leave' }).click();
});

test('peer connection rebuild is triggered from the UI recovery hook', async ({ browser }) => {
  const participant = await createRoom(browser, 'Recovery user');
  await participant.page.evaluate(() => window.__forcePeerRecovery());
  await expect(participant.page.locator('#meetingStatus')).toContainText('Rebuilding connection');
  await expect.poll(async () => participant.page.evaluate(() => window.__peerConnections.length), { timeout: 20000 }).toBeGreaterThan(1);
  await participant.page.getByRole('button', { name: 'Leave' }).click();
});

for (const deniedDevice of ['audio', 'video']) {
  test(`${deniedDevice} permission denial leaves the other capture usable`, async ({ browser }) => {
    const name = `No ${deniedDevice}`;
    const participant = await newMeetingPage(browser, name, deniedDevice);
    await participant.page.getByRole('button', { name: 'Create room' }).click();
    await expect(participant.page.locator('#meeting')).toBeVisible();
    await expect(participant.page.locator('#meetingStatus')).toContainText(
      deniedDevice === 'audio' ? 'Microphone unavailable' : 'Camera unavailable',
    );
    const tracks = await participant.page.locator('.tile video[aria-label="Your camera preview"]').evaluate((video) =>
      video.srcObject.getTracks().map((track) => track.kind),
    );
    expect(tracks).toContain(deniedDevice === 'audio' ? 'video' : 'audio');
    expect(tracks).not.toContain(deniedDevice);
    await expect(participant.page.locator('#micToggle')).toHaveAttribute('aria-pressed', deniedDevice === 'audio' ? 'false' : 'true');
    await expect(participant.page.locator('#cameraToggle')).toHaveAttribute('aria-pressed', deniedDevice === 'video' ? 'false' : 'true');
    await participant.page.getByRole('button', { name: 'Leave' }).click();
    await participant.context.close();
  });
}

test('meeting controls expose keyboard focus and status announcements at mobile width', async ({ browser }) => {
  const participant = await createRoom(browser, 'Mobile Guest');
  await participant.page.setViewportSize({ width: 390, height: 844 });
  const dimensions = await participant.page.evaluate(() => ({
    viewport: document.documentElement.clientWidth,
    content: document.documentElement.scrollWidth,
    liveStatuses: [...document.querySelectorAll('[role="status"][aria-live="polite"]')].length,
  }));
  expect(dimensions.content).toBeLessThanOrEqual(dimensions.viewport);
  expect(dimensions.liveStatuses).toBeGreaterThanOrEqual(2);
  await participant.page.keyboard.press('Tab');
  const focusIsVisible = await participant.page.evaluate(() => document.activeElement.matches(':focus-visible'));
  expect(focusIsVisible).toBe(true);
  await participant.page.getByRole('button', { name: 'Leave' }).click();
  await participant.context.close();
});