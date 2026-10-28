#include <xkbcommon/xkbcommon.h>
#include <stdio.h>
#include <string.h>

int main(void)
{
    struct xkb_context *ctx = xkb_context_new(XKB_CONTEXT_NO_DEFAULT_INCLUDES | XKB_CONTEXT_NO_ENVIRONMENT_NAMES);
    const char *keymap_text =
        "xkb_keymap {\n"
        "  xkb_keycodes { <AE01> = 10; <AC01> = 38; <LFSH> = 50; };\n"
        "  xkb_types { type \"ONE_LEVEL\" { modifiers = none; level_name[Level1] = \"Any\"; }; type \"ALPHABETIC\" { modifiers = Shift+Lock; map[Shift] = Level2; map[Lock] = Level2; level_name[Level1] = \"Base\"; level_name[Level2] = \"Caps\"; }; type \"TWO_LEVEL\" { modifiers = Shift; map[Shift] = Level2; level_name[Level1] = \"Base\"; level_name[Level2] = \"Shift\"; }; };\n"
        "  xkb_compatibility { interpret Shift_L { action = SetMods(modifiers=Shift); }; };\n"
        "  xkb_symbols { key <AE01> { [ 1, exclam ] }; key <AC01> { type = \"ALPHABETIC\", [ a, A ] }; key <LFSH> { [ Shift_L ] }; modifier_map Shift { <LFSH> }; };\n"
        "};\n";
    struct xkb_keymap *keymap = ctx ? xkb_keymap_new_from_string(ctx, keymap_text, XKB_KEYMAP_FORMAT_TEXT_V1, 0) : NULL;
    if (!keymap) {
        printf("FAIL xkbcommon keymap compile\n");
        return 1;
    }
    struct xkb_state *state = xkb_state_new(keymap);
    char plain[8] = { 0 }, shifted[8] = { 0 };
    xkb_state_key_get_utf8(state, 38, plain, sizeof plain);
    xkb_state_update_key(state, 50, XKB_KEY_DOWN);
    xkb_state_key_get_utf8(state, 38, shifted, sizeof shifted);
    int ok = strcmp(plain, "a") == 0 && strcmp(shifted, "A") == 0;
    printf("%s xkbcommon keymap: a -> %s, Shift+a -> %s\n", ok ? "PASS" : "FAIL", plain, shifted);
    xkb_state_unref(state);
    xkb_keymap_unref(keymap);
    xkb_context_unref(ctx);
    return ok ? 0 : 1;
}
