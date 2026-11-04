pub const INDEX_HTML: &str = r###"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>WebRTC Signaling Demo</title>
    <style>
        body {
            font-family: Arial, sans-serif;
            margin: 2rem;
            background: #0f172a;
            color: #e2e8f0;
        }
        .container { max-width: 960px; margin: 0 auto; }
        .grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
            gap: 1rem;
        }
        .video-panel { position: relative; }
        video {
            width: 100%; max-height: 320px; background: #020617;
            border: 1px solid #334155; border-radius: 12px;
        }
        button {
            background: #38bdf8; border: none; border-radius: 8px;
            color: #082f49; padding: 0.75rem 1rem; font-weight: 700;
            cursor: pointer; margin-right: 0.5rem;
        }
        button.media-control { min-width: 8rem; }
        button.icon-control {
            position: absolute; top: 0.75rem; right: 0.75rem; z-index: 1;
            min-width: 0; width: 2.75rem; height: 2.75rem; padding: 0;
            margin: 0; border: 1px solid rgba(255, 255, 255, 0.35);
            border-radius: 50%; background: rgba(15, 23, 42, 0.82); color: #f8fafc;
        }
        button.icon-control .control-icon { margin: 0; }
        button.icon-control .control-label {
            position: absolute; width: 1px; height: 1px; padding: 0;
            margin: -1px; overflow: hidden; clip: rect(0, 0, 0, 0);
            white-space: nowrap; border: 0;
        }
        button.icon-control.off { background: rgba(185, 28, 28, 0.9); }
        button.media-control.off { background: #475569; color: #e2e8f0; }
        .control-icon {
            display: inline-block; width: 1.25rem; margin-right: 0.35rem;
            font-size: 1.1rem; vertical-align: -0.1rem;
        }
        textarea {
            width: 100%; min-height: 120px; border-radius: 10px;
            border: 1px solid #334155; background: #020617; color: #e2e8f0;
            padding: 0.75rem; box-sizing: border-box; margin-top: 0.75rem;
        }
        #status { margin-top: 1rem; color: #7dd3fc; }
    </style>
</head>
<body>
    <div id="app" class="container">
        <h1>WebRTC Signaling Demo</h1>
        <p>Open this page in two tabs to test call negotiation through the Axum signaling server.</p>

        <div class="grid">
            <div class="video-panel">
                <video ref="localVideo" autoplay muted playsinline></video>
                <button
                    class="media-control icon-control"
                    :class="{ off: !micEnabled }"
                    type="button"
                    :title="micEnabled ? 'Turn off microphone' : 'Turn on microphone'"
                    aria-label="Toggle microphone"
                    :aria-pressed="String(micEnabled)"
                    @click="toggleMicrophone"
                >
                    <span class="control-icon" aria-hidden="true">{{ micEnabled ? '🎙' : '🔇' }}</span>
                    <span class="control-label">Mic {{ micEnabled ? "on" : "off" }}</span>
                </button>
            </div>
            <div
                class="video-panel"
                v-for="(remote, index) in remoteStreams"
                :key="remote.id"
            >
                <video
                    autoplay
                    playsinline
                    :ref="(element) => setRemoteVideo(element, remote.stream)"
                ></video>
                <span class="participant-label">Participant {{ index + 1 }}</span>
            </div>
        </div>

        <div style="margin-top: 1rem;">
            <button type="button" @click="startCall">Start Call</button>
            <button
                class="media-control"
                :class="{ off: !cameraEnabled }"
                type="button"
                :title="cameraEnabled ? 'Turn off camera' : 'Turn on camera'"
                :aria-pressed="String(cameraEnabled)"
                @click="toggleCamera"
            >
                <span class="control-icon" aria-hidden="true">{{ cameraEnabled ? '📹' : '🚫' }}</span>
                <span class="control-label">Camera {{ cameraEnabled ? 'on' : 'off' }}</span>
            </button>
            <button
                class="media-control"
                :class="{ off: sharingScreen }"
                type="button"
                :title="sharingScreen ? 'Stop sharing screen' : 'Share screen'"
                :aria-pressed="String(sharingScreen)"
                @click="toggleScreenSharing"
            >
                <span class="control-icon" aria-hidden="true">{{ sharingScreen ? '⏹' : '🖥' }}</span>
                <span class="control-label">{{ sharingScreen ? 'Stop sharing' : 'Share screen' }}</span>
            </button>
        </div>

        <div id="status" role="status" aria-live="polite">{{ status }}</div>
        <textarea ref="logArea" :value="logs" readonly aria-label="Connection log"></textarea>
    </div>

    <script src="https://unpkg.com/vue@3/dist/vue.global.prod.js"></script>
    <script>
        const { createApp, nextTick, onBeforeUnmount, onMounted, ref } = Vue;

        createApp({
            setup() {
                const room = 'demo-room';
                const localVideo = ref(null);
                const remoteStreams = ref([]);
                const logArea = ref(null);
                const status = ref('Connecting to signaling server...');
                const logs = ref('');
                const micEnabled = ref(true);
                const cameraEnabled = ref(true);
                const sharingScreen = ref(false);

                let peerId = null;
                let ws;
                let peerConnection;
                let localStream;
                let cameraTrack;
                let screenStream;

                function appendLog(message) {
                    const time = new Date().toLocaleTimeString();
                    logs.value += `[${time}] ${message}\n`;
                    nextTick(() => {
                        if (logArea.value) {
                            logArea.value.scrollTop = logArea.value.scrollHeight;
                        }
                    });
                }

                function updateMediaControls() {
                    micEnabled.value = Boolean(localStream?.getAudioTracks()[0]?.enabled);
                    cameraEnabled.value = Boolean(localStream?.getVideoTracks()[0]?.enabled);
                    sharingScreen.value = Boolean(screenStream);
                }

                function setRemoteVideo(element, stream) {
                    if (element && element.srcObject !== stream) element.srcObject = stream;
                }

                async function initMedia() {
                    localStream = await navigator.mediaDevices.getUserMedia({ video: true, audio: true });
                    cameraTrack = localStream.getVideoTracks()[0];
                    localVideo.value.srcObject = localStream;
                    updateMediaControls();
                }

                function configurePeerConnection() {
                    peerConnection = new RTCPeerConnection({
                        iceServers: [{ urls: 'stun:stun.l.google.com:19302' }],
                    });
                    localStream.getTracks().forEach((track) => peerConnection.addTrack(track, localStream));
                    peerConnection.ontrack = (event) => {
                        const stream = event.streams[0] ?? new MediaStream([event.track]);
                        const streamPrefix = `relay-${room}-`;
                        const participantId = stream.id.startsWith(streamPrefix)
                            ? stream.id.slice(streamPrefix.length)
                            : stream.id;
                        if (!remoteStreams.value.some((remote) => remote.id === stream.id)) {
                            remoteStreams.value.push({ id: stream.id, peerId: participantId, stream });
                        }
                        appendLog('Remote participant media received.');
                    };
                    peerConnection.onicecandidate = (event) => {
                        if (!event.candidate) return;
                        ws.send(JSON.stringify({
                            type: 'candidate', room, from: peerId, to: '',
                            candidate: {
                                candidate: event.candidate.candidate,
                                sdpMid: event.candidate.sdpMid,
                                sdpMLineIndex: event.candidate.sdpMLineIndex,
                            },
                        }));
                        appendLog('ICE candidate sent.');
                    };
                    peerConnection.onconnectionstatechange = () => {
                        status.value = `Connection state: ${peerConnection.connectionState}`;
                        appendLog(`Connection state changed to ${peerConnection.connectionState}.`);
                    };
                }

                async function createOffer() {
                    if (!peerConnection) configurePeerConnection();
                    const offer = await peerConnection.createOffer();
                    await peerConnection.setLocalDescription(offer);
                    ws.send(JSON.stringify({ type: 'offer', room, from: peerId, to: '', sdp: offer.sdp }));
                    appendLog('Offer sent.');
                }

                async function replaceVideoTrack(track) {
                    const sender = peerConnection?.getSenders().find((item) => item.track?.kind === 'video');
                    if (!sender) return;
                    await sender.replaceTrack(track);
                    localStream.removeTrack(localStream.getVideoTracks()[0]);
                    localStream.addTrack(track);
                    localVideo.value.srcObject = localStream;
                    await createOffer();
                }

                async function stopScreenSharing() {
                    if (!screenStream) return;
                    const previousScreenStream = screenStream;
                    screenStream = null;
                    previousScreenStream.getTracks().forEach((track) => track.stop());
                    await replaceVideoTrack(cameraTrack);
                    updateMediaControls();
                    appendLog('Screen sharing stopped.');
                }

                async function startScreenSharing() {
                    if (!peerConnection) configurePeerConnection();
                    screenStream = await navigator.mediaDevices.getDisplayMedia({ video: true });
                    const screenTrack = screenStream.getVideoTracks()[0];
                    screenTrack.addEventListener('ended', () => {
                        stopScreenSharing().catch((error) => appendLog(error.message));
                    }, { once: true });
                    await replaceVideoTrack(screenTrack);
                    updateMediaControls();
                    appendLog('Screen sharing started.');
                }

                async function handleSignal(event) {
                    const data = JSON.parse(event.data);
                    if (!data || !data.type) return;
                    if (data.type !== 'welcome' && data.to && data.to !== peerId) return;
                    appendLog(`Received signal: ${data.type}`);

                    if (data.type === 'peer-joined') {
                        status.value = 'Peer connected through the media server.';
                        return;
                    }
                    if (data.type === 'peer-left') {
                        remoteStreams.value = remoteStreams.value.filter(
                            (remote) => remote.peerId !== data.peer_id,
                        );
                        appendLog(`Participant ${data.peer_id} left.`);
                        return;
                    }
                    if (data.type === 'relay-error') {
                        status.value = `Media relay error: ${data.sdp}`;
                        appendLog(`Media relay error: ${data.sdp}`);
                        return;
                    }
                    if (data.type === 'welcome') {
                        peerId = data.peer_id;
                        status.value = `Connected as ${peerId}.`;
                        appendLog(`Assigned peer ID ${peerId}.`);
                        if (localStream) createOffer().catch((error) => appendLog(error.message));
                        return;
                    }
                    if (data.type === 'offer') {
                        if (!peerConnection) configurePeerConnection();
                        await peerConnection.setRemoteDescription({ type: 'offer', sdp: data.sdp });
                        const answer = await peerConnection.createAnswer();
                        await peerConnection.setLocalDescription(answer);
                        ws.send(JSON.stringify({ type: 'answer', room, from: peerId, to: data.from, sdp: answer.sdp }));
                        appendLog('Answer sent.');
                        return;
                    }
                    if (data.type === 'answer') {
                        await peerConnection.setRemoteDescription({ type: 'answer', sdp: data.sdp });
                        appendLog('Remote answer applied.');
                        return;
                    }
                    if (data.type === 'candidate' && data.candidate) {
                        if (!peerConnection) configurePeerConnection();
                        await peerConnection.addIceCandidate({
                            candidate: data.candidate.candidate,
                            sdpMid: data.candidate.sdpMid,
                            sdpMLineIndex: data.candidate.sdpMLineIndex,
                        });
                        appendLog('ICE candidate added.');
                    }
                }

                async function startCall() {
                    if (!peerId) {
                        appendLog('Waiting for the server to assign a peer ID.');
                        return;
                    }
                    if (!localStream) await initMedia();
                    if (!peerConnection) configurePeerConnection();
                    await createOffer();
                }

                function toggleMicrophone() {
                    const track = localStream?.getAudioTracks()[0];
                    if (track) {
                        track.enabled = !track.enabled;
                        updateMediaControls();
                    }
                }

                function toggleCamera() {
                    const track = localStream?.getVideoTracks()[0];
                    if (track) {
                        track.enabled = !track.enabled;
                        updateMediaControls();
                    }
                }

                async function toggleScreenSharing() {
                    try {
                        if (screenStream) await stopScreenSharing();
                        else await startScreenSharing();
                    } catch (error) {
                        appendLog(error.message);
                        updateMediaControls();
                    }
                }

                onMounted(() => {
                    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
                    ws = new WebSocket(`${protocol}//${location.host}/ws?room=${room}`);
                    ws.onopen = async () => {
                        status.value = 'Connected to signaling server.';
                        try {
                            await initMedia();
                        } catch (error) {
                            status.value = 'Microphone/camera access denied. Grant permissions and refresh.';
                            appendLog(error.message);
                        }
                    };
                    ws.onmessage = (event) => {
                        handleSignal(event).catch((error) => appendLog(error.message));
                    };
                    ws.onclose = () => {
                        status.value = 'WebSocket disconnected.';
                        appendLog('Connection closed. Refresh the page to reconnect.');
                    };
                });

                onBeforeUnmount(() => {
                    ws?.close();
                    peerConnection?.close();
                    localStream?.getTracks().forEach((track) => track.stop());
                    screenStream?.getTracks().forEach((track) => track.stop());
                });

                return {
                    cameraEnabled, logs, localVideo, logArea, micEnabled, remoteStreams,
                    sharingScreen, startCall, status, toggleCamera, toggleMicrophone,
                    toggleScreenSharing, setRemoteVideo,
                };
            },
        }).mount('#app');
    </script>
</body>
</html>
"###;
