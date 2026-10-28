#include <glib.h>
#include <glib/gstdio.h>
#include <stdio.h>
#include <string.h>

static int failures;

static void check(int ok, const char *what)
{
    printf("%s %s\n", ok ? "PASS" : "FAIL", what);
    if (!ok)
        failures++;
}

static gboolean on_timeout(gpointer data)
{
    int *ticks = data;
    (*ticks)++;
    return *ticks < 3;
}

static gboolean quit_loop(gpointer data)
{
    g_main_loop_quit(data);
    return G_SOURCE_REMOVE;
}

static void add_one(gpointer item, gpointer data)
{
    g_atomic_int_add((gint *)data, GPOINTER_TO_INT(item));
}

int main(void)
{
    GHashTable *table = g_hash_table_new_full(g_str_hash, g_str_equal, g_free, NULL);
    for (int i = 0; i < 1000; i++)
        g_hash_table_insert(table, g_strdup_printf("key%d", i), GINT_TO_POINTER(i));
    check(g_hash_table_size(table) == 1000 && GPOINTER_TO_INT(g_hash_table_lookup(table, "key777")) == 777, "glib hash table");
    g_hash_table_destroy(table);

    GString *text = g_string_new("hamix");
    g_string_append_printf(text, " %d.%d", 0, 7);
    g_string_ascii_up(text);
    check(strcmp(text->str, "HAMIX 0.7") == 0, "glib GString");
    g_string_free(text, TRUE);

    GRegex *regex = g_regex_new("^(\\w+)@(\\w+)$", 0, 0, NULL);
    GMatchInfo *match = NULL;
    gboolean matched = regex && g_regex_match(regex, "user@hamix", 0, &match);
    gchar *host = matched ? g_match_info_fetch(match, 2) : NULL;
    check(host && strcmp(host, "hamix") == 0, "glib GRegex (pcre2)");
    g_free(host);
    if (match)
        g_match_info_free(match);
    if (regex)
        g_regex_unref(regex);

    GKeyFile *keys = g_key_file_new();
    const char *ini = "[session]\nshell=/usr/bin/bash\nautostart=false\n";
    gboolean loaded = g_key_file_load_from_data(keys, ini, -1, 0, NULL);
    gchar *shell = loaded ? g_key_file_get_string(keys, "session", "shell", NULL) : NULL;
    check(shell && strcmp(shell, "/usr/bin/bash") == 0, "glib GKeyFile");
    g_free(shell);
    g_key_file_free(keys);

    int ticks = 0;
    GMainLoop *loop = g_main_loop_new(NULL, FALSE);
    g_timeout_add(10, on_timeout, &ticks);
    g_timeout_add(200, quit_loop, loop);
    gint64 started = g_get_monotonic_time();
    g_main_loop_run(loop);
    gint64 elapsed = g_get_monotonic_time() - started;
    check(ticks == 3 && elapsed >= 150000, "glib main loop with timeouts");
    g_main_loop_unref(loop);

    gint sum = 0;
    GThreadPool *pool = g_thread_pool_new(add_one, &sum, 4, FALSE, NULL);
    for (int i = 1; i <= 100; i++)
        g_thread_pool_push(pool, GINT_TO_POINTER(i), NULL);
    g_thread_pool_free(pool, FALSE, TRUE);
    check(sum == 5050, "glib thread pool");

    gchar *out = NULL;
    gint status = -1;
    gchar *argv[] = { "/usr/bin/uname", "-s", NULL };
    gboolean spawned = g_spawn_sync(NULL, argv, NULL, 0, NULL, NULL, &out, NULL, &status, NULL);
    check(spawned && status == 0 && out && g_str_has_prefix(out, "HamixOS"), "glib g_spawn_sync");
    g_free(out);

    gchar *path = g_build_filename(g_get_tmp_dir(), "glib-test.txt", NULL);
    gboolean written = g_file_set_contents(path, "hello glib\n", -1, NULL);
    gchar *contents = NULL;
    gboolean read_back = written && g_file_get_contents(path, &contents, NULL, NULL);
    check(read_back && strcmp(contents, "hello glib\n") == 0, "glib file contents");
    g_free(contents);
    g_unlink(path);
    g_free(path);

    printf("glib %u.%u.%u: %d failure(s)\n", glib_major_version, glib_minor_version, glib_micro_version, failures);
    return failures ? 1 : 0;
}
