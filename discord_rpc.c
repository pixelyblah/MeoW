#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <stdint.h>
#include <signal.h>
#include <poll.h>

#define CLIENT_ID "1545372090859921438"
#define MAX_BUF 65536

static volatile int running = 1;
void handle_signal(int sig) { (void)sig; running = 0; }

int connect_discord(void) {
    const char *rt = getenv("XDG_RUNTIME_DIR");
    if (!rt) rt = "/tmp";
    for (int i = 0; i < 10; i++) {
        char path[256];
        snprintf(path, sizeof(path), "%s/discord-ipc-%d", rt, i);
        int fd = socket(AF_UNIX, SOCK_STREAM, 0);
        if (fd < 0) continue;
        struct sockaddr_un addr = { .sun_family = AF_UNIX };
        strncpy(addr.sun_path, path, sizeof(addr.sun_path) - 1);
        if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) == 0) {
            printf("[*] Connected to %s\n", path);
            return fd;
        }
        close(fd);
    }
    return -1;
}

int send_frame(int fd, uint32_t opcode, const char *payload, uint32_t len) {
    uint8_t header[8];
    header[0] = (opcode >> 0) & 0xFF;
    header[1] = (opcode >> 8) & 0xFF;
    header[2] = (opcode >> 16) & 0xFF;
    header[3] = (opcode >> 24) & 0xFF;
    header[4] = (len >> 0) & 0xFF;
    header[5] = (len >> 8) & 0xFF;
    header[6] = (len >> 16) & 0xFF;
    header[7] = (len >> 24) & 0xFF;
    if (send(fd, header, 8, MSG_NOSIGNAL) != 8) return -1;
    if (len > 0 && send(fd, payload, len, MSG_NOSIGNAL) != (ssize_t)len) return -1;
    return 0;
}

int send_raw(int fd, uint32_t opcode, const char *json) {
    return send_frame(fd, opcode, json, strlen(json));
}

int recv_frame(int fd, uint8_t *out_opcode, char *out_payload, uint32_t *out_len) {
    uint8_t header[8];
    ssize_t n = 0;
    while (n < 8) {
        ssize_t r = recv(fd, header + n, 8 - n, 0);
        if (r <= 0) return -1;
        n += r;
    }
    *out_opcode = header[0];
    *out_len = header[4] | (header[5] << 8) | (header[6] << 16) | (header[7] << 24);
    if (*out_len > MAX_BUF) return -1;
    n = 0;
    while ((uint32_t)n < *out_len) {
        ssize_t r = recv(fd, out_payload + n, *out_len - n, 0);
        if (r <= 0) return -1;
        n += r;
    }
    out_payload[n] = '\0';
    return 0;
}

int send_json(int fd, uint32_t opcode, const char *json) {
    uint32_t len = strlen(json);
    return send_frame(fd, opcode, json, len);
}

int do_handshake(int fd) {
    char buf[512];
    snprintf(buf, sizeof(buf), "{\"v\":1,\"client_id\":\"%s\"}", CLIENT_ID);
    return send_json(fd, 0, buf);
}

int set_activity(int fd, const char *state, const char *details,
                 const char *large_text, const char *small_text,
                 const char *button_label, const char *button_url) {
    char payload[MAX_BUF];
    char buttons[512] = "";
    if (button_label && button_url && button_label[0] && button_url[0]) {
        snprintf(buttons, sizeof(buttons),
            ",\"buttons\":[{\"label\":\"%s\",\"url\":\"%s\"}]",
            button_label, button_url);
    }
    snprintf(payload, sizeof(payload),
        "{\"nonce\":\"%ld\",\"cmd\":\"SET_ACTIVITY\",\"args\":{"
        "\"pid\":%d,\"activity\":{"
        "\"state\":\"%s\","
        "\"details\":\"%s\","
        "\"assets\":{\"large_text\":\"%s\",\"small_text\":\"%s\"}"
        "%s}}}",
        (long)time(NULL) ^ getpid(), getpid(),
        state, details, large_text, small_text, buttons);
    return send_json(fd, 1, payload);
}

int clear_activity(int fd) {
    char payload[256];
    snprintf(payload, sizeof(payload),
        "{\"nonce\":\"%ld\",\"cmd\":\"SET_ACTIVITY\",\"args\":{\"pid\":%d,\"activity\":null}}",
        (long)time(NULL) ^ getpid(), getpid());
    return send_json(fd, 1, payload);
}

int read_all(int fd) {
    uint8_t opcode;
    char payload[MAX_BUF];
    uint32_t len;
    struct pollfd pfd = { .fd = fd, .events = POLLIN };
    if (poll(&pfd, 1, 100) <= 0) return 0;
    if (recv_frame(fd, &opcode, payload, &len) < 0) return -1;
    if (opcode == 1) {
        if (strstr(payload, "\"READY\"")) printf("[*] Ready\n");
        else if (strstr(payload, "\"ACTIVITY_JOIN\"")) printf("[*] Join event\n");
    } else if (opcode == 3) {
        send_frame(fd, 4, "{}", 2);
    } else if (opcode == 6) {
        fprintf(stderr, "[!] Error: %s\n", payload);
        return -1;
    }
    return 0;
}

int main(int argc, char *argv[]) {
    signal(SIGINT, handle_signal);
    signal(SIGTERM, handle_signal);

    int fd = connect_discord();
    if (fd < 0) {
        fprintf(stderr, "[!] Discord IPC not found. Is Discord running?\n");
        return 1;
    }

    if (do_handshake(fd) < 0) {
        fprintf(stderr, "[!] Handshake send failed\n");
        close(fd);
        return 1;
    }
    printf("[*] Handshake sent\n");

    int ready = 0;
    for (int i = 0; i < 50 && !ready; i++) {
        uint8_t opcode;
        char payload[MAX_BUF];
        uint32_t len;
        if (recv_frame(fd, &opcode, payload, &len) < 0) {
            fprintf(stderr, "[!] No response from Discord\n");
            close(fd);
            return 1;
        }
        if (opcode == 1 && strstr(payload, "\"READY\"")) {
            printf("[*] Ready\n");
            ready = 1;
        } else if (opcode == 6) {
            fprintf(stderr, "[!] Handshake rejected: %s\n", payload);
            close(fd);
            return 1;
        }
    }
    if (!ready) {
        fprintf(stderr, "[!] Timeout waiting for READY\n");
        close(fd);
        return 1;
    }

    const char *state   = argc > 1 ? argv[1] : "Playing a game";
    const char *details = argc > 2 ? argv[2] : "In the terminal";
    const char *ltext   = argc > 3 ? argv[3] : "Large";
    const char *stext   = argc > 4 ? argv[4] : "Small";
    const char *blabel  = argc > 5 ? argv[5] : "";
    const char *burl    = argc > 6 ? argv[6] : "";

    if (set_activity(fd, state, details, ltext, stext, blabel, burl) < 0) {
        fprintf(stderr, "[!] Failed to set activity\n");
        close(fd);
        return 1;
    }
    printf("[*] Activity set: %s - %s\n", state, details);

    while (running) {
        if (read_all(fd) < 0) { fprintf(stderr, "[!] Connection lost\n"); break; }
        sleep(15);
        if (set_activity(fd, state, details, ltext, stext, blabel, burl) < 0) {
            fprintf(stderr, "[!] Heartbeat failed\n");
            break;
        }
    }

    clear_activity(fd);
    close(fd);
    printf("[*] Goodbye\n");
    return 0;
}
