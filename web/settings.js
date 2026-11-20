// Campus Crypt: saves the settings (volume, reduced motion, toggles) in localStorage
// (src/settings.rs). Without this script the game uses its defaults.
miniquad_add_plugin({
    register_plugin: function (importObject) {
        importObject.env.cc_setting_get = function (id) {
            try {
                var v = window.localStorage.getItem("campus_crypt_setting_" + id);
                return v === null ? -1 : parseFloat(v);
            } catch (e) {
                return -1;
            }
        };
        importObject.env.cc_setting_set = function (id, v) {
            try {
                window.localStorage.setItem("campus_crypt_setting_" + id, String(v));
            } catch (e) { }
        };
    },
    name: "cc_settings",
    version: 1,
});
