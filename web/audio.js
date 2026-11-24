"use strict";

const AudioContext = window.AudioContext || window.webkitAudioContext;
let audio_context;
let sounds = new Map();
let playbacks = [];
let sound_key_next = 1;
let playback_key_next = 1;

// Mobile Safari leaves AudioContext suspended until resume() runs inside a
// user gesture. These listeners are installed at load, in the capture phase,
// so the first tap or key counts even when it arrives before audio_init and
// before the canvas calls preventDefault (which can swallow the gesture).
const UNLOCK_EVENTS = ["touchstart", "touchend", "pointerdown", "mousedown", "keydown", "keyup", "keypress"];

function ensure_audio_context() {
    const AC = window.AudioContext || window.webkitAudioContext;
    if (!AC) return null;
    if (audio_context == null) {
        audio_context = new AC();
    }
    return audio_context;
}

function unlock_web_audio() {
    const ctx = ensure_audio_context();
    if (!ctx) return;
    if (ctx.resume) {
        const pending = ctx.resume();
        if (pending && pending.catch) pending.catch(function () {});
    }
    // Older Safari only starts audio after a buffer plays on the same context,
    // from inside the gesture.
    try {
        const buffer = ctx.createBuffer(1, 1, 22050);
        const source = ctx.createBufferSource();
        source.buffer = buffer;
        source.connect(ctx.destination);
        if (source.start) source.start(0);
        else if (source.noteOn) source.noteOn(0);
    } catch (e) {}
    for (const type of UNLOCK_EVENTS) {
        document.removeEventListener(type, unlock_web_audio, true);
    }
}

for (const type of UNLOCK_EVENTS) {
    document.addEventListener(type, unlock_web_audio, true);
}

function audio_init() {
    ensure_audio_context();
}

function audio_add_buffer(content, content_len) {
    let content_array = wasm_memory.buffer.slice(content, content + content_len);

    let sound_key = sound_key_next;
    sound_key_next += 1;

    audio_context.decodeAudioData(content_array, function(buffer) {
        sounds.set(sound_key, buffer);
    }, function(e) {
        // fail
        console.error("Failed to decode audio buffer", e);
    });
    return sound_key;
}

function audio_source_is_loaded(sound_key) {
    return sounds.has(sound_key) && sounds.get(sound_key) != undefined;
}

function recycle_playback() {
    let playback = playbacks.find(playback => playback.sound_key === 0);

    if (playback != null) {
        playback.source = audio_context.createBufferSource();
    } else {
        playback = {
            sound_key: 0,
            playback_key: 0,
            source: audio_context.createBufferSource(),
            gain_node: audio_context.createGain(),
            ended: null,
        };

        playbacks.push(playback);
    }

    return playback;
}

function stop(playback) {
    try {
        playback.source.removeEventListener('ended', playback.ended);

        playback.source.disconnect();
        playback.gain_node.disconnect();

        playback.sound_key = 0;
        playback.playback_key = 0;
    } catch (e) {
        console.error("Error stopping sound", e);
    }
}

function audio_play_buffer(sound_key, volume, repeat) {
    let playback_key = playback_key_next++;

    let pb = recycle_playback();

    pb.sound_key = sound_key;
    pb.playback_key = playback_key;

    pb.source.connect(pb.gain_node);
    pb.gain_node.connect(audio_context.destination);

    pb.gain_node.gain.value = volume;
    pb.source.loop = repeat;

    pb.ended = function() {
        stop(pb);
    };
    pb.source.addEventListener('ended', pb.ended);

    try {
        pb.source.buffer = sounds.get(sound_key);
        pb.source.start(0);
    } catch (e) {
        console.error("Error starting sound", e);
    }

    return playback_key;
}

function audio_source_set_volume(sound_key, volume) {
    playbacks.forEach(playback => {
        if (playback.sound_key === sound_key) {
            playback.gain_node.gain.value = volume;
        }
    });
}

function audio_source_stop(sound_key) {
    playbacks.forEach(playback => {
        playback.sound_key === sound_key && stop(playback);
    });
}

function audio_source_delete(sound_key) {
    audio_source_stop(sound_key);

    sounds.delete(sound_key);
}

function audio_playback_stop(playback_key) {
    let playback = playbacks.find(playback => playback.playback_key === playback_key);

    playback != null && stop(playback);
}

function audio_playback_set_volume(playback_key, volume) {
    let playback = playbacks.find(playback => playback.playback_key === playback_key);

    if (playback != null) {
        playback.gain_node.gain.value = volume;
    }
}

function register_plugin(importObject) {
    importObject.env.audio_init = audio_init;
    importObject.env.audio_add_buffer = audio_add_buffer;
    importObject.env.audio_play_buffer = audio_play_buffer;
    importObject.env.audio_source_is_loaded = audio_source_is_loaded;
    importObject.env.audio_source_set_volume = audio_source_set_volume;
    importObject.env.audio_source_stop = audio_source_stop;
    importObject.env.audio_source_delete = audio_source_delete;
    importObject.env.audio_playback_stop = audio_playback_stop;
    importObject.env.audio_playback_set_volume = audio_playback_set_volume;
}

miniquad_add_plugin({ register_plugin, version: 1, name: "macroquad_audio" });
