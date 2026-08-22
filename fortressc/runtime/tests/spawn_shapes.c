/*
 * Does the spawn queue satisfy the four corpus shapes, at the SHIM level,
 * before any compiler wiring exists?
 *
 * Each case below is one of ProjectFortress/tests/Spawn*.fss transcribed into
 * C against the same shims generated code will call. They are here rather than
 * in a .fss gate because the failure mode of every one of them is a HANG, and
 * a hang is cheapest to bisect with no compiler in the loop.
 *
 * THE ARGUMENT THESE FOUR MAKE TOGETHER is that no drain policy works and a
 * real concurrent context is required:
 *   spawn2  progress with NO join before the parent's spin
 *   spawn3  the child must NOT be run to completion at the spawn site
 *   spawn6  `ready()` must be false immediately after spawning long work
 *   spawn5  a value comes back through val()
 * spawn2 against spawn3+spawn6 is the contradiction; it has no inline solution.
 *
 *   cc -I runtime runtime/tests/spawn_shapes.c runtime/shims.c -lgc -lm -pthread \
 *      -o spawn-shapes
 *
 * Every case is run under an alarm, because a wrong answer here does not
 * return: it never returns.
 */
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

void fortress_runtime_init(void);
void *fortress_spawn(void *(*body)(void *), void *env);
void *fortress_thread_val(void *handle);
void fortress_thread_wait(void *handle);
int fortress_thread_ready(void *handle);
void fortress_thread_stop(void *handle);

/* The captured environment, exactly the shape the outliner builds: a
 * by-reference capture is the ADDRESS of the parent's storage. */
struct env {
    volatile long *x;
    volatile long *y;
    long count;
};

static int failures = 0;
static const char *current = "none";

static void on_alarm(int sig) {
    (void)sig;
    /* write(2), not printf: this runs in a signal handler. */
    const char *msg = "HUNG   ";
    ssize_t ignored = write(2, msg, strlen(msg));
    ignored += write(2, current, strlen(current));
    ignored += write(2, "\n", 1);
    (void)ignored;
    _exit(9);
}

static void ok(const char *name, int cond) {
    if (cond) {
        printf("ok    %s\n", name);
    } else {
        printf("FAIL  %s\n", name);
        failures++;
    }
}

/* ---- Spawn1: the child runs at all, and wait() sees it ---- */
static void *body_set_x(void *raw) {
    struct env *e = raw;
    *e->x = 1;
    return NULL;
}

static void case_spawn1(void) {
    volatile long x = 0;
    struct env e = {&x, NULL, 0};
    void *t = fortress_spawn(body_set_x, &e);
    fortress_thread_wait(t);
    ok("spawn1  wait() sees the child's store", x == 1);
    fortress_thread_stop(t);
}

/* ---- Spawn2: NO join before the parent spins. This is the case that
 *      forbids running a spawned body only at a join point. ---- */
static void *body_set_xy(void *raw) {
    struct env *e = raw;
    *e->x = 1;
    *e->y = 1;
    return NULL;
}

static void case_spawn2(void) {
    volatile long x = 0, y = 0;
    struct env e = {&x, &y, 0};
    void *t = fortress_spawn(body_set_xy, &e);
    while (x == 0) {
        /* the parent's spin, with nothing draining any queue */
    }
    ok("spawn2  the parent's spin terminates with no join", x == 1);
    ok("spawn2  both stores are visible", y == 1);
    fortress_thread_stop(t);
}

/* ---- Spawn3: the CHILD spins on a value the parent sets after the spawn.
 *      Running the body to completion at the spawn site is a hang. ---- */
static void *body_spin_until_x(void *raw) {
    struct env *e = raw;
    *e->y = 1;
    while (*e->x == 0) {
    }
    return NULL;
}

static void case_spawn3(void) {
    volatile long x = 0, y = 0;
    struct env e = {&x, &y, 0};
    void *t = fortress_spawn(body_spin_until_x, &e);
    x = 1;
    fortress_thread_wait(t);
    ok("spawn3  the child saw the parent's later store", y == 1);
    fortress_thread_stop(t);
}

/* ---- Spawn5: a value comes back ---- */
static void *body_count(void *raw) {
    struct env *e = raw;
    long sum = 0;
    for (long i = 0; i < e->count; i++) {
        sum++;
    }
    return (void *)(intptr_t)sum;
}

static void case_spawn5(void) {
    struct env e = {NULL, NULL, 10};
    void *t = fortress_spawn(body_count, &e);
    long got = (long)(intptr_t)fortress_thread_val(t);
    ok("spawn5  val() returns the body's value", got == 10);
    fortress_thread_stop(t);
}

/* ---- Spawn6: ready() is FALSE immediately after spawning long work.
 *      Running the body at the spawn site makes it true. ---- */
static void case_spawn6(void) {
    struct env e = {NULL, NULL, 200000000L};
    void *t = fortress_spawn(body_count, &e);
    int ready = fortress_thread_ready(t);
    ok("spawn6  ready() is false while the body is in flight", ready == 0);
    fortress_thread_stop(t);
}

/* ---- val() must still work when the runner is busy forever: the queued task
 *      is STOLEN onto the calling thread. Nothing in the corpus writes this and
 *      it is the property the steal path exists for. ---- */
static void *body_forever(void *raw) {
    struct env *e = raw;
    while (*e->x == 0) {
    }
    return NULL;
}

static void case_steal(void) {
    volatile long never = 0;
    struct env blocking = {&never, NULL, 0};
    void *hog = fortress_spawn(body_forever, &blocking);
    struct env work = {NULL, NULL, 7};
    void *t = fortress_spawn(body_count, &work);
    long got = (long)(intptr_t)fortress_thread_val(t);
    ok("steal   a queued task runs on the caller when the runner is busy",
       got == 7);
    never = 1;
    fortress_thread_stop(hog);
    fortress_thread_stop(t);
}

struct case_entry {
    const char *name;
    void (*run)(void);
};

int main(void) {
    static const struct case_entry cases[] = {
        {"spawn1", case_spawn1}, {"spawn2", case_spawn2}, {"spawn3", case_spawn3},
        {"spawn5", case_spawn5}, {"spawn6", case_spawn6}, {"steal", case_steal},
    };
    fortress_runtime_init();
    signal(SIGALRM, on_alarm);
    for (size_t i = 0; i < sizeof cases / sizeof cases[0]; i++) {
        current = cases[i].name;
        alarm(20);
        cases[i].run();
        alarm(0);
    }
    printf("%s\n", failures == 0 ? "SPAWN SHAPES GREEN" : "SPAWN SHAPES FAILED");
    return failures == 0 ? 0 : 1;
}
