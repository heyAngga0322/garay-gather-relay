pub const INDEX_HTML: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta name="theme-color" content="#101513" />
    <title>Gather | Meeting room</title>
    <style>
        :root {
            color-scheme: dark;
            --page: #101513;
            --surface: #1a211e;
            --surface-raised: #242d29;
            --surface-soft: #303b35;
            --line: #3a4740;
            --ink: #f1f5f0;
            --muted: #aab8ae;
            --accent: #bce36a;
            --accent-ink: #19220e;
            --danger: #ff887b;
            --focus: #f5dc82;
            font-family: "Trebuchet MS", "Segoe UI", sans-serif;
        }
        * { box-sizing: border-box; }
        body { margin: 0; min-height: 100vh; background: var(--page); color: var(--ink); }
        button, input { font: inherit; }
        button { cursor: pointer; }
        button:focus-visible, input:focus-visible, [tabindex]:focus-visible {
            outline: 3px solid var(--focus);
            outline-offset: 3px;
        }
        .topbar {
            min-height: 68px; display: flex; align-items: center; justify-content: space-between;
            padding: 0 28px; border-bottom: 1px solid var(--line); background: #151b18;
        }
        .brand { display: flex; align-items: center; gap: 10px; font-size: 17px; font-weight: 800; }
        .brand-mark { width: 14px; aspect-ratio: 1; background: var(--accent); border-radius: 4px 10px 4px 10px; transform: rotate(-12deg); }
        .eyebrow { color: var(--muted); font-size: 12px; text-transform: uppercase; letter-spacing: 1px; }
        .entry {
            width: min(100% - 32px, 560px); margin: clamp(42px, 9vh, 100px) auto;
            padding: clamp(24px, 5vw, 42px); background: var(--surface);
            border: 1px solid var(--line); border-radius: 8px;
        }
        h1, h2, p { margin-top: 0; }
        .entry h1 { margin-bottom: 8px; font-size: 32px; }
        .entry-copy { color: var(--muted); margin-bottom: 28px; }
        .form-grid { display: grid; gap: 18px; }
        label { display: grid; gap: 8px; color: var(--muted); font-size: 14px; }
        input {
            width: 100%; min-height: 46px; padding: 10px 12px; border: 1px solid var(--line);
            border-radius: 5px; background: #111714; color: var(--ink);
        }
        .actions, .control-group { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
        .button {
            min-height: 42px; padding: 9px 14px; border: 1px solid transparent; border-radius: 5px;
            background: var(--accent); color: var(--accent-ink); font-weight: 750;
        }
        .button:hover { filter: brightness(1.08); }
        .button.secondary { background: var(--surface-soft); color: var(--ink); border-color: var(--line); }
        .button.danger { background: #5a2926; color: #fff0ed; border-color: #88413a; }
        .button[aria-pressed="false"] { background: var(--surface-soft); color: var(--muted); border-color: var(--line); }
        .notice { min-height: 24px; margin: 18px 0 0; color: var(--muted); }
        .notice[data-error="true"] { color: var(--danger); }
        .meeting { min-height: calc(100vh - 68px); display: flex; flex-direction: column; }
        .meeting-head {
            min-height: 66px; padding: 12px 24px; display: flex; align-items: center;
            justify-content: space-between; gap: 16px; border-bottom: 1px solid var(--line);
        }
        .meeting-title { display: flex; align-items: baseline; gap: 12px; min-width: 0; }
        .meeting-title h1 { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; margin: 0; font-size: 19px; }
        .room-code { color: var(--muted); font-size: 13px; }
        .workspace { flex: 1; display: grid; grid-template-columns: minmax(0, 1fr) 270px; min-height: 0; }
        .stage { min-width: 0; padding: 22px; display: flex; align-items: center; justify-content: center; }
        .media-grid {
            width: min(100%, 1180px); min-height: 280px; display: grid; gap: 12px;
            grid-template-columns: repeat(auto-fit, minmax(min(100%, 320px), 1fr)); align-content: center;
        }
        .tile {
            position: relative; min-width: 0; aspect-ratio: 16 / 9; overflow: hidden;
            background: #202a25; border: 1px solid var(--line); border-radius: 6px;
        }
        .tile video { display: block; width: 100%; height: 100%; object-fit: cover; background: #18201c; }
        .tile video.mirror { transform: scaleX(-1); }
        .tile-info {
            position: absolute; inset: auto 0 0; display: flex; align-items: center;
            justify-content: space-between; gap: 10px; padding: 34px 12px 11px;
            background: linear-gradient(transparent, rgb(0 0 0 / 78%));
        }
        .tile-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 14px; font-weight: 700; }
        .tile-state { color: #e5eee7; font-size: 12px; }
        .pin-button { min-width: 34px; min-height: 32px; padding: 4px 8px; color: var(--ink); background: #202a25; border: 1px solid #64736a; border-radius: 4px; }
        .tile.is-pinned { border: 2px solid var(--accent); }
        .tile.is-speaking { border-color: #f5dc82; box-shadow: 0 0 0 1px #f5dc82; }
        .media-grid[data-layout="speaker"] .tile { display: none; }
        .media-grid[data-layout="speaker"] .tile.is-featured { display: block; grid-column: 1 / -1; width: min(100%, 980px); justify-self: center; }
        .roster {
            padding: 22px 18px; border-left: 1px solid var(--line); background: #151b18;
        }
        .roster-head { display: flex; justify-content: space-between; align-items: baseline; gap: 10px; margin-bottom: 14px; }
        .roster-head h2 { margin: 0; font-size: 16px; }
        .roster-count { color: var(--muted); font-size: 12px; }
        .participant-list { list-style: none; margin: 0; padding: 0; display: grid; gap: 4px; }
        .participant { min-height: 48px; display: flex; align-items: center; gap: 10px; padding: 7px 4px; border-bottom: 1px solid #2c3731; }
        .avatar { flex: 0 0 30px; width: 30px; aspect-ratio: 1; display: grid; place-items: center; border-radius: 50%; background: #3d5144; color: #e3f1df; font-size: 13px; font-weight: 800; }
        .participant-copy { min-width: 0; flex: 1; }
        .participant-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; font-weight: 700; }
        .participant-state { color: var(--muted); font-size: 11px; margin-top: 3px; }
        .controls {
            min-height: 76px; padding: 12px 18px; display: flex; align-items: center;
            justify-content: center; gap: 20px; background: #151b18; border-top: 1px solid var(--line);
        }
        .control-button { min-width: 86px; }
        .segmented { display: inline-flex; gap: 3px; padding: 3px; border: 1px solid var(--line); border-radius: 6px; background: #101513; }
        .segmented button { min-height: 34px; padding: 6px 11px; border: 0; border-radius: 4px; background: transparent; color: var(--muted); }
        .segmented button[aria-pressed="true"] { background: var(--surface-soft); color: var(--ink); }
        .device-picker { display: grid; gap: 6px; color: var(--muted); font-size: 12px; }
        .device-picker select {
            min-height: 36px; padding: 5px 10px; border: 1px solid var(--line); border-radius: 5px;
            background: #111714; color: var(--ink);
        }
        .hidden { display: none !important; }
        @media (max-width: 760px) {
            .topbar { min-height: 58px; padding: 0 16px; }
            .workspace { grid-template-columns: 1fr; }
            .stage { padding: 14px; }
            .media-grid { grid-template-columns: repeat(auto-fit, minmax(min(100%, 260px), 1fr)); }
            .roster { border-left: 0; border-top: 1px solid var(--line); padding: 14px 18px; }
            .participant-list { grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); }
            .meeting-head { padding: 10px 14px; }
            .controls { gap: 9px; flex-wrap: wrap; }
            .control-button { min-width: 72px; }
        }
        @media (prefers-reduced-motion: no-preference) {
            .entry { animation: arrive 260ms ease-out both; }
            @keyframes arrive { from { opacity: 0; transform: translateY(8px); } to { opacity: 1; transform: translateY(0); } }
        }
    </style>
</head>
<body>
    <header class="topbar">
        <div class="brand"><span class="brand-mark" aria-hidden="true"></span>Gather</div>
        <span class="eyebrow">Small rooms, together</span>
    </header>

    <main>
        <section id="entry" class="entry" aria-labelledby="entryTitle">
            <p class="eyebrow">A quiet place to meet</p>
            <h1 id="entryTitle">Join the conversation</h1>
            <p class="entry-copy">Start a room or enter with an invite from your host.</p>
            <form id="meetingForm" class="form-grid">
                <label for="displayName">Your name
                    <input id="displayName" name="displayName" type="text" maxlength="64" autocomplete="name" placeholder="Alex Morgan" required />
                </label>
                <label for="roomInput">Room code or invite link
                    <input id="roomInput" name="room" type="text" autocomplete="url" placeholder="Paste an invite link or room code" />
                </label>
                <div class="actions">
                    <button id="createMeeting" class="button" type="button">Create room</button>
                    <button id="joinMeeting" class="button secondary" type="submit">Join room</button>
                </div>
            </form>
            <p id="entryStatus" class="notice" role="status" aria-live="polite"></p>
        </section>

        <section id="meeting" class="meeting hidden" aria-label="Meeting room">
            <header class="meeting-head">
                <div class="meeting-title"><h1 id="meetingTitle">Meeting</h1><span id="roomCode" class="room-code"></span></div>
                <button id="copyInvite" class="button secondary" type="button" aria-label="Copy meeting invite">Copy invite</button>
            </header>
            <div class="workspace">
                <section class="stage" aria-label="Participant videos">
                    <div id="mediaGrid" class="media-grid" data-layout="grid" aria-live="off"></div>
                </section>
                <aside class="roster" aria-labelledby="rosterTitle">
                    <div class="roster-head"><h2 id="rosterTitle">People</h2><span id="rosterCount" class="roster-count">0 here</span></div>
                    <ul id="participantList" class="participant-list"></ul>
                </aside>
            </div>
            <footer class="controls" aria-label="Meeting controls">
                <div class="control-group">
                    <button id="micToggle" class="button control-button" type="button" aria-label="Turn microphone off" aria-pressed="true">Mic on</button>
                    <button id="cameraToggle" class="button control-button" type="button" aria-label="Turn camera off" aria-pressed="true">Camera on</button>
                    <button id="screenShareToggle" class="button secondary control-button" type="button" aria-label="Share screen" aria-pressed="false">Share screen</button>
                </div>
                <div class="control-group">
                    <label class="device-picker" for="microphoneDevice">Mic input
                        <select id="microphoneDevice" aria-label="Microphone device"></select>
                    </label>
                    <label class="device-picker" for="cameraDevice">Camera
                        <select id="cameraDevice" aria-label="Camera device"></select>
                    </label>
                </div>
                <div class="segmented" role="group" aria-label="Video layout">
                    <button type="button" data-layout-choice="grid" aria-pressed="true">Grid</button>
                    <button type="button" data-layout-choice="speaker" aria-pressed="false">Speaker</button>
                </div>
                <button id="leaveMeeting" class="button danger control-button" type="button">Leave</button>
            </footer>
            <p id="meetingStatus" class="notice" role="status" aria-live="polite"></p>
        </section>
    </main>

    <script>
        const byId = (id) => document.getElementById(id);
        const entry = byId('entry');
        const meeting = byId('meeting');
        const displayNameInput = byId('displayName');
        const roomInput = byId('roomInput');
        const entryStatus = byId('entryStatus');
        const meetingStatus = byId('meetingStatus');
        const mediaGrid = byId('mediaGrid');
        const participantList = byId('participantList');
        const participants = new Map();
        const remoteMedia = new Map();
        let currentRoom = '';
        let currentName = '';
        let currentPeerId = '';
        let currentRole = 'guest';
        let socket;
        let signalQueue = Promise.resolve();
        let peerConnection;
        let localStream = new MediaStream();
        let pinnedPeerId = '';
        let activePeerId = '';
        let mediaNotice = '';
        let audioContext;
        let speakerFrame = 0;
        let microphoneDeviceId = '';
        let cameraDeviceId = '';
        let screenShareStream = null;
        let screenShareActive = false;
        let currentAudioTrack = null;
        let currentCameraTrack = null;
        let reconnectAttempts = 0;
        let reconnectTimer = null;
        let isReconnecting = false;
        let isLeavingMeeting = false;
        let peerRebuildInProgress = false;

        function setStatus(target, text, isError = false) {
            target.textContent = text;
            target.dataset.error = String(isError);
        }

        function roomFromValue(value) {
            const trimmed = value.trim();
            if (!trimmed) return '';
            try {
                const url = new URL(trimmed);
                return url.searchParams.get('room') || url.pathname.split('/').filter(Boolean).pop() || '';
            } catch {
                return trimmed;
            }
        }

        function inviteUrl() {
            return `${location.origin}/?room=${encodeURIComponent(currentRoom)}`;
        }

        function participantState(peerId) {
            return participants.get(peerId) || {
                peer_id: peerId,
                display_name: peerId === currentPeerId ? currentName : 'Joining...',
                muted: false,
                camera_on: true,
                screen_share: false,
                connected: true,
            };
        }

        function mediaTile(peerId, isLocal = false) {
            let tile = document.querySelector(`[data-peer-id="${CSS.escape(peerId)}"]`);
            if (tile) return tile;

            tile = document.createElement('article');
            tile.className = 'tile';
            tile.dataset.peerId = peerId;
            const video = document.createElement('video');
            video.autoplay = true;
            video.playsInline = true;
            video.muted = isLocal;
            video.setAttribute('aria-label', isLocal ? 'Your camera preview' : 'Participant video');
            if (isLocal) video.classList.add('mirror');
            const info = document.createElement('div');
            info.className = 'tile-info';
            const name = document.createElement('span');
            name.className = 'tile-name';
            const state = document.createElement('span');
            state.className = 'tile-state';
            const pin = document.createElement('button');
            pin.className = 'pin-button';
            pin.type = 'button';
            pin.textContent = 'Pin';
            pin.setAttribute('aria-label', 'Pin participant video');
            pin.setAttribute('aria-pressed', 'false');
            pin.addEventListener('click', () => togglePin(peerId));
            info.append(name, state, pin);
            tile.append(video, info);
            mediaGrid.append(tile);
            return tile;
        }

        function updateTile(peerId) {
            const item = participantState(peerId);
            const tile = mediaTile(peerId, peerId === currentPeerId);
            tile.querySelector('.tile-name').textContent = peerId === currentPeerId ? `${item.display_name} (you)` : item.display_name;
            const labels = [];
            if (item.muted) labels.push('Mic off');
            if (!item.camera_on) labels.push('Camera off');
            if (item.screen_share) labels.push('Sharing screen');
            tile.querySelector('.tile-state').textContent = labels.join(' · ');
            const pin = tile.querySelector('.pin-button');
            pin.setAttribute('aria-pressed', String(pinnedPeerId === peerId));
            pin.setAttribute('aria-label', `${pinnedPeerId === peerId ? 'Unpin' : 'Pin'} ${item.display_name}`);
            tile.classList.toggle('is-pinned', pinnedPeerId === peerId);
            tile.classList.toggle('is-speaking', activePeerId === peerId);
            tile.classList.toggle('is-featured', (pinnedPeerId || activePeerId || currentPeerId) === peerId);
            const remote = remoteMedia.get(peerId);
            if (remote && tile.querySelector('video').srcObject !== remote.stream) {
                tile.querySelector('video').srcObject = remote.stream;
            }
        }

        function renderRoster() {
            participantList.replaceChildren();
            for (const [peerId, item] of participants) {
                if (!item.connected) continue;
                const row = document.createElement('li');
                row.className = 'participant';
                const avatar = document.createElement('span');
                avatar.className = 'avatar';
                avatar.setAttribute('aria-hidden', 'true');
                avatar.textContent = (item.display_name || '?').trim().slice(0, 1).toUpperCase();
                const copy = document.createElement('div');
                copy.className = 'participant-copy';
                const name = document.createElement('div');
                name.className = 'participant-name';
                name.textContent = peerId === currentPeerId ? `${item.display_name} (you)` : item.display_name;
                const state = document.createElement('div');
                state.className = 'participant-state';
                const labels = [item.role === 'host' ? 'Host' : 'Guest'];
                labels.push(item.muted ? 'Mic off' : 'Mic on');
                labels.push(item.camera_on ? 'Camera on' : 'Camera off');
                if (item.screen_share) labels.push('Sharing screen');
                state.textContent = labels.join(' / ');
                copy.append(name, state);
                row.append(avatar, copy);
                participantList.append(row);
                updateTile(peerId);
            }
            byId('rosterCount').textContent = `${participantList.children.length} here`;
        }

        function applyRoster(snapshot) {
            const next = new Map(snapshot.filter((item) => item.connected !== false).map((item) => [item.peer_id, item]));
            if (currentPeerId && !next.has(currentPeerId)) {
                next.set(currentPeerId, participantState(currentPeerId));
            }
            for (const peerId of participants.keys()) {
                if (peerId !== currentPeerId && !next.has(peerId)) removeParticipant(peerId);
            }
            participants.clear();
            for (const [peerId, item] of next) participants.set(peerId, item);
            renderRoster();
        }

        function removeParticipant(peerId) {
            participants.delete(peerId);
            const remote = remoteMedia.get(peerId);
            if (remote) {
                for (const track of remote.stream.getTracks()) track.stop();
                remoteMedia.delete(peerId);
                remote.analyser?.disconnect();
                remote.source?.disconnect();
            }
            document.querySelector(`[data-peer-id="${CSS.escape(peerId)}"]`)?.remove();
            if (pinnedPeerId === peerId) pinnedPeerId = '';
            if (activePeerId === peerId) activePeerId = '';
            renderRoster();
        }

        function togglePin(peerId) {
            pinnedPeerId = pinnedPeerId === peerId ? '' : peerId;
            for (const id of participants.keys()) updateTile(id);
            if (mediaGrid.dataset.layout === 'speaker') setFeaturedTile();
        }

        function setFeaturedTile() {
            const featured = pinnedPeerId || activePeerId || currentPeerId;
            for (const tile of mediaGrid.children) tile.classList.toggle('is-featured', tile.dataset.peerId === featured);
        }

        function sendSignal(signal) {
            if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(signal));
        }

        function publishLocalState() {
            const audioTrack = localStream.getAudioTracks()[0];
            const videoTrack = localStream.getVideoTracks()[0];
            const muted = !audioTrack || !audioTrack.enabled;
            const cameraOn = Boolean(videoTrack?.enabled);
            const local = participants.get(currentPeerId);
            if (local) {
                local.muted = muted;
                local.camera_on = cameraOn;
                local.screen_share = screenShareActive;
                updateTile(currentPeerId);
                renderRoster();
            }
            sendSignal({
                type: 'state',
                display_name: currentName,
                muted,
                camera_on: cameraOn,
                screen_share: screenShareActive,
            });
        }

        function setToggle(button, enabled, name, onText, offText) {
            button.textContent = enabled ? onText : offText;
            button.setAttribute('aria-pressed', String(enabled));
            button.setAttribute('aria-label', `${enabled ? 'Turn' : 'Turn'} ${name} ${enabled ? 'off' : 'on'}`);
        }

        function queueReconnect() {
            if (isLeavingMeeting || meeting.classList.contains('hidden') || isReconnecting) return;
            const attemptNumber = reconnectAttempts + 1;
            reconnectAttempts = attemptNumber;
            isReconnecting = true;
            const delayMs = Math.min(500 * Math.pow(2, reconnectAttempts - 1), 8000);
            setStatus(meetingStatus, `Reconnecting to the meeting (${attemptNumber}/5)...`, false);
            clearTimeout(reconnectTimer);
            reconnectTimer = setTimeout(async () => {
                try {
                    const freshTicket = await requestJson(`/api/meeting/${encodeURIComponent(currentRoom)}/ticket`, {
                        display_name: currentName,
                    });
                    if (!freshTicket?.ticket) throw new Error('No fresh ticket received.');
                    await connectToRoom(currentRoom, freshTicket.ticket, currentName, { reconnect: true });
                    isReconnecting = false;
                    reconnectAttempts = 0;
                } catch (error) {
                    if (reconnectAttempts >= 5) {
                        isReconnecting = false;
                        setStatus(meetingStatus, 'Connection failed. Leave and rejoin to continue.', true);
                        return;
                    }
                    queueReconnect();
                }
            }, delayMs);
        }

        async function rebuildPeerConnection() {
            if (peerRebuildInProgress || meeting.classList.contains('hidden') || !currentRoom || isLeavingMeeting) return;
            peerRebuildInProgress = true;
            setStatus(meetingStatus, 'Media connection degraded. Rebuilding connection...', true);
            if (peerConnection) {
                peerConnection.close();
                peerConnection = undefined;
            }
            await makePeerConnection();
            if (peerConnection && socket?.readyState === WebSocket.OPEN) {
                const offer = await peerConnection.createOffer();
                await peerConnection.setLocalDescription(offer);
                sendSignal({ type: 'offer', sdp: offer.sdp });
            }
            peerRebuildInProgress = false;
        }

        if (typeof window !== 'undefined') {
            window.__forcePeerRecovery = async () => {
                await rebuildPeerConnection();
            };
        }

        function setScreenShareButtonState() {
            const button = byId('screenShareToggle');
            button.textContent = screenShareActive ? 'Stop sharing' : 'Share screen';
            button.setAttribute('aria-pressed', String(screenShareActive));
            button.setAttribute('aria-label', screenShareActive ? 'Stop sharing screen' : 'Share screen');
        }

        async function populateDeviceChoices() {
            const audioSelect = byId('microphoneDevice');
            const cameraSelect = byId('cameraDevice');
            if (!audioSelect || !cameraSelect || !navigator.mediaDevices || !navigator.mediaDevices.enumerateDevices) {
                return;
            }
            try {
                const devices = await navigator.mediaDevices.enumerateDevices();
                const audioDevices = devices.filter((device) => device.kind === 'audioinput');
                const videoDevices = devices.filter((device) => device.kind === 'videoinput');
                const buildOptions = (select, items, fallbackText) => {
                    const currentValue = select.value || '';
                    select.innerHTML = '';
                    const fallback = document.createElement('option');
                    fallback.value = '';
                    fallback.textContent = fallbackText;
                    select.append(fallback);
                    for (const device of items) {
                        const option = document.createElement('option');
                        option.value = device.deviceId || '';
                        option.textContent = device.label || `Device ${device.deviceId || 'Unknown'}`;
                        select.append(option);
                    }
                    if (currentValue && [...select.options].some((option) => option.value === currentValue)) {
                        select.value = currentValue;
                    } else if (items[0]) {
                        select.value = items[0].deviceId || '';
                    } else {
                        select.value = '';
                    }
                };
                buildOptions(audioSelect, audioDevices, 'Default microphone');
                buildOptions(cameraSelect, videoDevices, 'Default camera');
                microphoneDeviceId = audioDevices[0]?.deviceId || '';
                cameraDeviceId = videoDevices[0]?.deviceId || '';
            } catch {
                microphoneDeviceId = '';
                cameraDeviceId = '';
            }
        }

        async function refreshLocalMedia() {
            if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
                return;
            }
            try {
                const requestedAudio = microphoneDeviceId ? { deviceId: { exact: microphoneDeviceId } } : true;
                const requestedVideo = cameraDeviceId ? { deviceId: { exact: cameraDeviceId } } : true;
                const stream = await navigator.mediaDevices.getUserMedia({
                    audio: requestedAudio,
                    video: requestedVideo,
                });
                const nextAudio = stream.getAudioTracks()[0] || null;
                const nextVideo = stream.getVideoTracks()[0] || null;
                for (const track of localStream.getTracks()) track.stop();
                localStream = new MediaStream();
                if (nextAudio) {
                    localStream.addTrack(nextAudio);
                    currentAudioTrack = nextAudio;
                }
                if (nextVideo) {
                    localStream.addTrack(nextVideo);
                    currentCameraTrack = nextVideo;
                }
                const localTile = mediaTile('local-preview', true);
                localTile.dataset.peerId = 'local-preview';
                localTile.querySelector('video').srcObject = localStream;
                if (peerConnection) {
                    const sender = peerConnection.getSenders().find((candidate) => candidate.track?.kind === 'video');
                    if (sender) {
                        sender.replaceTrack(screenShareActive && screenShareStream?.getVideoTracks()[0] ? screenShareStream.getVideoTracks()[0] : currentCameraTrack || null);
                    }
                }
                const audioOn = Boolean(currentAudioTrack?.enabled);
                const videoOn = Boolean(currentCameraTrack?.enabled);
                setToggle(byId('micToggle'), audioOn, 'microphone', 'Mic on', 'Mic off');
                setToggle(byId('cameraToggle'), videoOn, 'camera', 'Camera on', 'Camera off');
                publishLocalState();
            } catch (error) {
                setStatus(meetingStatus, `Could not switch media devices: ${error.message || 'Permission denied'}.`, true);
            }
        }

        async function captureLocalMedia() {
            localStream = new MediaStream();
            const outcomes = await Promise.all([
                ['audio', {
                    audio: microphoneDeviceId ? { deviceId: { exact: microphoneDeviceId } } : true,
                    video: false,
                }],
                ['video', {
                    audio: false,
                    video: cameraDeviceId ? { deviceId: { exact: cameraDeviceId } } : true,
                }],
            ].map(async ([kind, constraints]) => {
                try {
                    const stream = await navigator.mediaDevices.getUserMedia(constraints);
                    for (const track of stream.getTracks()) {
                        localStream.addTrack(track);
                        if (kind === 'audio') currentAudioTrack = track;
                        if (kind === 'video') currentCameraTrack = track;
                    }
                    return { kind, ok: true };
                } catch (error) {
                    return { kind, ok: false, error };
                }
            }));
            await populateDeviceChoices();
            const localTile = mediaTile('local-preview', true);
            localTile.dataset.peerId = 'local-preview';
            localTile.querySelector('video').srcObject = localStream;
            localTile.querySelector('.tile-name').textContent = `${currentName} (you)`;
            localTile.querySelector('.tile-state').textContent = '';
            localTile.querySelector('.pin-button').hidden = true;
            const audio = outcomes.find((result) => result.kind === 'audio');
            const video = outcomes.find((result) => result.kind === 'video');
            const audioOn = audio.ok && localStream.getAudioTracks().length > 0;
            const videoOn = video.ok && localStream.getVideoTracks().length > 0;
            setToggle(byId('micToggle'), audioOn, 'microphone', 'Mic on', 'Mic off');
            setToggle(byId('cameraToggle'), videoOn, 'camera', 'Camera on', 'Camera off');
            if (!audioOn && !videoOn) mediaNotice = 'Microphone and camera are unavailable. You can still join and listen.';
            else if (!audioOn || !videoOn) mediaNotice = `${audioOn ? 'Camera' : 'Microphone'} unavailable. Continuing with ${audioOn ? 'audio' : 'video'} only.`;
            if (mediaNotice) setStatus(meetingStatus, mediaNotice, !audioOn && !videoOn);
        }

        async function stopScreenShare(showStatus = true) {
            if (screenShareStream) {
                for (const track of screenShareStream.getTracks()) track.stop();
                screenShareStream = null;
            }
            screenShareActive = false;
            setScreenShareButtonState();
            const sender = peerConnection?.getSenders().find((candidate) => candidate.track?.kind === 'video');
            if (sender && currentCameraTrack) sender.replaceTrack(currentCameraTrack);
            if (showStatus) setStatus(meetingStatus, 'Screen sharing stopped.');
            publishLocalState();
        }

        async function startScreenShare() {
            if (!navigator.mediaDevices || !navigator.mediaDevices.getDisplayMedia) {
                setStatus(meetingStatus, 'Screen sharing is not available in this browser.', true);
                return;
            }
            try {
                const displayStream = await navigator.mediaDevices.getDisplayMedia({
                    video: { frameRate: 30 },
                    audio: false,
                });
                screenShareStream = displayStream;
                screenShareActive = true;
                const screenTrack = displayStream.getVideoTracks()[0];
                if (screenTrack) {
                    const sender = peerConnection?.getSenders().find((candidate) => candidate.track?.kind === 'video');
                    if (sender) sender.replaceTrack(screenTrack);
                }
                setScreenShareButtonState();
                for (const track of displayStream.getVideoTracks()) {
                    track.addEventListener('ended', () => {
                        if (screenShareActive) stopScreenShare(false);
                    }, { once: true });
                }
                setStatus(meetingStatus, 'Screen sharing started.');
                publishLocalState();
            } catch (error) {
                setStatus(meetingStatus, `Screen sharing was cancelled: ${error.message || 'permission denied'}.`, true);
            }
        }

        async function makePeerConnection() {
            peerConnection = new RTCPeerConnection({
                iceServers: [{ urls: ['stun:stun.l.google.com:19302'] }],
            });
            for (const track of localStream.getTracks()) peerConnection.addTrack(track, localStream);
            peerConnection.addEventListener('icecandidate', (event) => {
                if (event.candidate) sendSignal({ type: 'candidate', candidate: event.candidate.toJSON() });
            });
            peerConnection.addEventListener('connectionstatechange', () => {
                const state = peerConnection.connectionState;
                if (state === 'connected') {
                    setStatus(meetingStatus, `Connected. Your media is live.${mediaNotice ? ` ${mediaNotice}` : ''}`, !localStream.getAudioTracks().length && !localStream.getVideoTracks().length);
                    return;
                }
                if (state === 'connecting') {
                    setStatus(meetingStatus, 'Connecting media...');
                    return;
                }
                if (state === 'failed' || state === 'disconnected') {
                    if (!isLeavingMeeting && !isReconnecting && !peerRebuildInProgress) {
                        rebuildPeerConnection();
                    }
                    return;
                }
                if (state === 'closed') {
                    if (!isLeavingMeeting && !isReconnecting && !peerRebuildInProgress) {
                        queueReconnect();
                    }
                }
            });
            peerConnection.addEventListener('track', (event) => {
                const streamId = event.streams[0]?.id || '';
                const prefix = `relay-${currentRoom}-`;
                const peerId = streamId.startsWith(prefix) ? streamId.slice(prefix.length) : '';
                if (!peerId) {
                    setStatus(meetingStatus, 'Received a media track without participant identity.', true);
                    return;
                }
                const remote = remoteMedia.get(peerId) || { stream: new MediaStream(), source: null, analyser: null, level: 0 };
                if (!remote.stream.getTracks().some((track) => track.id === event.track.id)) remote.stream.addTrack(event.track);
                remoteMedia.set(peerId, remote);
                if (!participants.has(peerId)) {
                    participants.set(peerId, { peer_id: peerId, display_name: peerId, role: 'guest', muted: false, camera_on: true, connected: true });
                }
                mediaTile(peerId).querySelector('video').srcObject = remote.stream;
                updateTile(peerId);
                if (event.track.kind === 'audio') watchSpeaker(peerId, remote);
                event.track.addEventListener('ended', () => {
                    remote.stream.removeTrack(event.track);
                    if (remote.stream.getTracks().length === 0) removeParticipant(peerId);
                }, { once: true });
                renderRoster();
            });
        }

        function watchSpeaker(peerId, remote) {
            try {
                audioContext ||= new AudioContext();
                audioContext.resume().catch(() => {});
                remote.source ||= audioContext.createMediaStreamSource(remote.stream);
                remote.analyser ||= audioContext.createAnalyser();
                remote.analyser.fftSize = 512;
                remote.source.connect(remote.analyser);
                if (!speakerFrame) speakerFrame = requestAnimationFrame(sampleSpeakerLevels);
                remoteMedia.set(peerId, remote);
            } catch {
                activePeerId ||= peerId;
            }
        }

        function sampleSpeakerLevels() {
            let loudestPeer = '';
            let loudestLevel = 0;
            for (const [peerId, remote] of remoteMedia) {
                if (!remote.analyser) continue;
                const samples = new Uint8Array(remote.analyser.fftSize);
                remote.analyser.getByteTimeDomainData(samples);
                let sum = 0;
                for (const sample of samples) {
                    const value = (sample - 128) / 128;
                    sum += value * value;
                }
                remote.level = Math.sqrt(sum / samples.length);
                if (remote.level > loudestLevel) {
                    loudestLevel = remote.level;
                    loudestPeer = peerId;
                }
            }
            if (loudestLevel > 0.035) activePeerId = loudestPeer;
            for (const peerId of participants.keys()) updateTile(peerId);
            setFeaturedTile();
            speakerFrame = requestAnimationFrame(sampleSpeakerLevels);
        }

        async function handleSignal(message) {
            if (message.type === 'welcome') {
                currentPeerId = message.peer_id;
                currentRole = message.role || 'guest';
                participants.set(currentPeerId, {
                    peer_id: currentPeerId, display_name: currentName, role: currentRole,
                    muted: !localStream.getAudioTracks().some((track) => track.enabled),
                    camera_on: localStream.getVideoTracks().some((track) => track.enabled), connected: true,
                });
                mediaGrid.querySelector('[data-peer-id="local-preview"]').dataset.peerId = currentPeerId;
                applyRoster(message.participants || []);
                participants.set(currentPeerId, { ...participants.get(currentPeerId), display_name: currentName, role: currentRole });
                mediaGrid.querySelector(`[data-peer-id="${CSS.escape(currentPeerId)}"] video`).setAttribute('aria-label', 'Your camera preview');
                publishLocalState();
                const offer = await peerConnection.createOffer();
                await peerConnection.setLocalDescription(offer);
                sendSignal({ type: 'offer', sdp: offer.sdp });
                return;
            }
            if (message.type === 'roster') {
                applyRoster(message.participants || []);
                return;
            }
            if (message.type === 'peer-joined') {
                if (message.peer_id && message.peer_id !== currentPeerId) {
                    participants.set(message.peer_id, {
                        peer_id: message.peer_id, display_name: message.display_name || message.peer_id,
                        role: message.role || 'guest', muted: message.muted, camera_on: message.camera_on,
                        screen_share: message.screen_share, connected: true,
                    });
                    renderRoster();
                }
                if (message.participants?.length) applyRoster(message.participants);
                return;
            }
            if (message.type === 'peer-left') {
                removeParticipant(message.peer_id);
                setStatus(meetingStatus, `${message.display_name || 'A participant'} left the meeting.`);
                return;
            }
            if (message.type === 'candidate' && message.candidate) {
                await peerConnection.addIceCandidate(message.candidate);
                return;
            }
            if (message.type === 'answer') {
                if (peerConnection.signalingState === 'have-local-offer') {
                    await peerConnection.setRemoteDescription({ type: 'answer', sdp: message.sdp });
                }
                return;
            }
            if (message.type === 'offer') {
                if (peerConnection.signalingState === 'have-local-offer') {
                    await peerConnection.setLocalDescription({ type: 'rollback' });
                }
                await peerConnection.setRemoteDescription({ type: 'offer', sdp: message.sdp });
                const answer = await peerConnection.createAnswer();
                await peerConnection.setLocalDescription(answer);
                sendSignal({ type: 'answer', sdp: answer.sdp });
                return;
            }
            if (message.type === 'relay-error') setStatus(meetingStatus, message.sdp || 'Media relay reported an error.', true);
            if (message.type === 'error') setStatus(meetingStatus, message.sdp || 'The meeting rejected an action.', true);
        }

        async function connectToRoom(room, ticket, name, options = {}) {
            currentRoom = room;
            currentName = name;
            isLeavingMeeting = false;
            isReconnecting = false;
            if (!options.reconnect) {
                reconnectAttempts = 0;
                clearTimeout(reconnectTimer);
            }
            byId('meetingTitle').textContent = `${name}'s meeting`;
            byId('roomCode').textContent = room;
            entry.classList.add('hidden');
            meeting.classList.remove('hidden');
            history.replaceState({}, '', `/?room=${encodeURIComponent(room)}`);
            if (options.reconnect) {
                participants.clear();
                remoteMedia.clear();
                mediaGrid.replaceChildren();
                currentPeerId = '';
                if (peerConnection) {
                    peerConnection.close();
                    peerConnection = undefined;
                }
                if (localStream.getTracks().length) {
                    const preview = mediaTile('local-preview', true);
                    preview.dataset.peerId = 'local-preview';
                    preview.querySelector('video').srcObject = localStream;
                    preview.querySelector('.tile-name').textContent = `${name} (you)`;
                    preview.querySelector('.tile-state').textContent = '';
                    preview.querySelector('.pin-button').hidden = true;
                }
            }
            if (!localStream.getTracks().length) {
                await captureLocalMedia();
            }
            if (!peerConnection) {
                await makePeerConnection();
            }
            const scheme = location.protocol === 'https:' ? 'wss:' : 'ws:';
            const url = `${scheme}//${location.host}/ws?room=${encodeURIComponent(room)}&ticket=${encodeURIComponent(ticket)}&display_name=${encodeURIComponent(name)}`;
            const activeSocket = new WebSocket(url);
            socket = activeSocket;
            activeSocket.addEventListener('open', () => {
                if (socket !== activeSocket) return;
                if (options.reconnect) {
                    setStatus(meetingStatus, 'Connection restored. Rejoining media...');
                } else {
                    setStatus(meetingStatus, 'Signaling connected. Joining media...');
                }
                if (peerConnection && peerConnection.signalingState === 'stable') {
                    const offer = peerConnection.createOffer();
                    peerConnection.setLocalDescription(offer).then(() => {
                        sendSignal({ type: 'offer', sdp: offer.sdp });
                    }).catch(() => {});
                }
            });
            activeSocket.addEventListener('message', (event) => {
                if (socket !== activeSocket) return;
                signalQueue = signalQueue.then(() => handleSignal(JSON.parse(event.data))).catch((error) => {
                    setStatus(meetingStatus, `Meeting signal failed: ${error.message}`, true);
                });
            });
            activeSocket.addEventListener('close', () => {
                if (socket !== activeSocket) return;
                if (!meeting.classList.contains('hidden') && !isLeavingMeeting && !isReconnecting) {
                    queueReconnect();
                }
            });
            activeSocket.addEventListener('error', () => {
                if (socket !== activeSocket) return;
                if (!meeting.classList.contains('hidden') && !isLeavingMeeting && !isReconnecting) {
                    queueReconnect();
                }
            });
        }

        async function requestJson(url, payload) {
            const response = await fetch(url, {
                method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload),
            });
            const data = await response.json();
            if (!response.ok) throw new Error(data.error || 'Meeting request failed.');
            return data;
        }

        async function createMeeting() {
            const name = displayNameInput.value.trim();
            if (!name) return setStatus(entryStatus, 'Enter your name to create a room.', true);
            try {
                setStatus(entryStatus, 'Creating room...');
                const data = await requestJson('/api/meeting', { display_name: name });
                sessionStorage.setItem(`host-ticket:${data.room_id}`, data.host_ticket);
                await connectToRoom(data.room_id, data.host_ticket, name);
            } catch (error) {
                setStatus(entryStatus, error.message, true);
            }
        }

        async function joinMeeting(event) {
            event.preventDefault();
            const name = displayNameInput.value.trim();
            const room = roomFromValue(roomInput.value);
            if (!name) return setStatus(entryStatus, 'Enter your name before joining.', true);
            if (!room) return setStatus(entryStatus, 'Enter a room code or invite link.', true);
            try {
                setStatus(entryStatus, 'Requesting a secure join ticket...');
                const data = await requestJson(`/api/meeting/${encodeURIComponent(room)}/ticket`, { display_name: name });
                await connectToRoom(room, data.ticket, name);
            } catch (error) {
                setStatus(entryStatus, error.message, true);
            }
        }

        async function copyInvite() {
            const invite = inviteUrl();
            try {
                await navigator.clipboard.writeText(invite);
                setStatus(meetingStatus, 'Invite link copied.');
            } catch {
                const field = document.createElement('input');
                field.value = invite;
                document.body.append(field);
                field.select();
                document.execCommand('copy');
                field.remove();
                setStatus(meetingStatus, 'Invite link copied.');
            }
        }

        function leaveMeeting() {
            isLeavingMeeting = true;
            isReconnecting = false;
            clearTimeout(reconnectTimer);
            reconnectTimer = null;
            reconnectAttempts = 0;
            if (speakerFrame) cancelAnimationFrame(speakerFrame);
            speakerFrame = 0;
            socket?.close();
            peerConnection?.close();
            for (const track of localStream.getTracks()) track.stop();
            for (const peerId of remoteMedia.keys()) removeParticipant(peerId);
            audioContext?.close().catch(() => {});
            socket = undefined;
            peerConnection = undefined;
            currentPeerId = '';
            participants.clear();
            remoteMedia.clear();
            mediaGrid.replaceChildren();
            meeting.classList.add('hidden');
            entry.classList.remove('hidden');
            setStatus(entryStatus, 'You left the meeting.');
        }

        byId('meetingForm').addEventListener('submit', joinMeeting);
        byId('createMeeting').addEventListener('click', createMeeting);
        byId('copyInvite').addEventListener('click', copyInvite);
        byId('leaveMeeting').addEventListener('click', leaveMeeting);
        byId('screenShareToggle').addEventListener('click', () => {
            if (screenShareActive) stopScreenShare();
            else startScreenShare();
        });
        byId('micToggle').addEventListener('click', (event) => {
            const track = localStream.getAudioTracks()[0];
            if (!track) return setStatus(meetingStatus, 'No microphone is available.', true);
            track.enabled = !track.enabled;
            setToggle(event.currentTarget, track.enabled, 'microphone', 'Mic on', 'Mic off');
            publishLocalState();
        });
        byId('cameraToggle').addEventListener('click', (event) => {
            const track = localStream.getVideoTracks()[0];
            if (!track) return setStatus(meetingStatus, 'No camera is available.', true);
            track.enabled = !track.enabled;
            setToggle(event.currentTarget, track.enabled, 'camera', 'Camera on', 'Camera off');
            publishLocalState();
        });
        byId('microphoneDevice').addEventListener('change', async (event) => {
            microphoneDeviceId = event.target.value || '';
            if (microphoneDeviceId) await refreshLocalMedia();
        });
        byId('cameraDevice').addEventListener('change', async (event) => {
            cameraDeviceId = event.target.value || '';
            if (cameraDeviceId) await refreshLocalMedia();
        });
        document.querySelectorAll('[data-layout-choice]').forEach((button) => {
            button.addEventListener('click', () => {
                mediaGrid.dataset.layout = button.dataset.layoutChoice;
                document.querySelectorAll('[data-layout-choice]').forEach((choice) => {
                    choice.setAttribute('aria-pressed', String(choice === button));
                });
                setFeaturedTile();
            });
        });

        const params = new URLSearchParams(location.search);
        const roomFromUrl = params.get('room') || '';
        if (roomFromUrl) roomInput.value = roomFromUrl;
        const savedName = localStorage.getItem('meeting-display-name');
        if (savedName) displayNameInput.value = savedName;
        displayNameInput.addEventListener('change', () => localStorage.setItem('meeting-display-name', displayNameInput.value.trim()));
        const hostTicket = roomFromUrl && sessionStorage.getItem(`host-ticket:${roomFromUrl}`);
        if (hostTicket) {
            connectToRoom(roomFromUrl, hostTicket, displayNameInput.value.trim() || 'Host').catch((error) => {
                setStatus(entryStatus, error.message, true);
            });
        }
    </script>
</body>
</html>
"###;
