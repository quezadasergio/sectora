/* Minimal POSIX shims for building GNU ddrescue with MinGW-w64. */
#ifndef SECTORA_MINGW_POSIX_H
#define SECTORA_MINGW_POSIX_H

#if defined(__MINGW32__) || defined(__MINGW64__)

#include <io.h>
#include <errno.h>
#include <signal.h>

#ifndef STDIN_FILENO
#define STDIN_FILENO 0
#endif
#ifndef STDOUT_FILENO
#define STDOUT_FILENO 1
#endif
#ifndef STDERR_FILENO
#define STDERR_FILENO 2
#endif

#ifndef TCIFLUSH
#define TCIFLUSH 0
#endif

static inline int sectora_fsync(int fd) { return _commit(fd); }
#define fsync sectora_fsync

static inline int tcflush(int, int) { return 0; }

struct termios {};

#ifndef _SC_PAGESIZE
#define _SC_PAGESIZE 30
#endif
static inline long sysconf(int name) {
  if (name == _SC_PAGESIZE) return 4096L;
  errno = EINVAL;
  return -1L;
}

#ifndef SIGHUP
#define SIGHUP 1
#endif
#ifndef SIGUSR1
#define SIGUSR1 30
#endif
#ifndef SIGUSR2
#define SIGUSR2 31
#endif
#ifndef SA_RESTART
#define SA_RESTART 0x10000000
#endif

typedef unsigned long sectora_sigset_t;
#ifndef sigset_t
#define sigset_t sectora_sigset_t
#endif

struct sigaction {
  void (*sa_handler)(int);
  sigset_t sa_mask;
  int sa_flags;
};

static inline int sigemptyset(sigset_t *set) {
  if (set) *set = 0;
  return 0;
}

static inline int sigaction(int signum, const struct sigaction *act, struct sigaction *) {
  if (!act) return -1;
  /* Map unsupported POSIX signals to no-ops on Windows. */
  if (signum == SIGHUP || signum == SIGUSR1 || signum == SIGUSR2) return 0;
  signal(signum, act->sa_handler);
  return 0;
}

#endif /* mingw */
#endif /* SECTORA_MINGW_POSIX_H */
