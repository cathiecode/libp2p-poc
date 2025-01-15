#include <stdio.h>
#include <unistd.h>
#include <pthread.h>
#include <string.h>
#include "artifact/libp2p_stream_poc.h"

void* read_thread_func(void *arg) {
    struct MirrorClient *mirror_client = (struct MirrorClient *)arg;

    while (1) {
        char buffer[1024];
        int result;
        if ((result = read_mirror_client(mirror_client, buffer, 0, 1024)) < 0) {
            printf("Error reading message: %d\n", result);
            break;
        }

        printf("Result of read: '%d`\n", result);

        buffer[result] = '\0';

        printf("Received message: '%s`\n", buffer);

        sleep(1);
    }

    printf("Exiting read thread\n");

    return NULL;
}

void* write_thread_func(void *arg) {
    struct MirrorClient *mirror_client = (struct MirrorClient *)arg;

    while (1) {
        char buffer[1024];

        if (fgets(buffer, sizeof buffer, stdin) == 0) {
            break;
        }

        sleep(1);
    }

    printf("Exiting write thread\n");

    return NULL;
}

int main(int argc, char **argv) {
    int result;

    if (argc < 2)  {
        printf("Usage: %s <peer>\n", argv[0]);
        return -1;
    }

    printf("Connecting to peer: %s\n", argv[2]);

    struct NetworkContext *context = create_context(argv[1]);

    struct MirrorClient *mirror_client;
    if ((result = connect_mirror(context, argv[2], &mirror_client)) < 0) {
        printf("Error connecting to peer: %d\n", result);
        return -1;
    }

    printf("Connected to peer\n");

    uint32_t send_counter = 0;
    uint8_t counter_as_bytes[4] = {0, 0, 0, 0};
    uint8_t received_counter_as_bytes[4] = {0, 0, 0, 0};

    while(1) {
        counter_as_bytes[0] = (send_counter >> 24) & 0xFF;
        counter_as_bytes[1] = (send_counter >> 16) & 0xFF;
        counter_as_bytes[2] = (send_counter >> 8) & 0xFF;
        counter_as_bytes[3] = send_counter & 0xFF;

        clock_t begin = clock();
        write_mirror_client(mirror_client, counter_as_bytes, 0, 4);

        read_mirror_client(mirror_client, received_counter_as_bytes, 0, 4);

        clock_t end = clock();

        double time_spent = (double)(end - begin);

        printf("Time spent: %fus\n", time_spent);

        uint32_t received_counter = (received_counter_as_bytes[0] << 24) | (received_counter_as_bytes[1] << 16) | (received_counter_as_bytes[2] << 8) | received_counter_as_bytes[3];
        if (send_counter != received_counter) {
            printf("Error: Sent counter: %d, Received counter: %d\n", send_counter, received_counter);
            break;
        }

        printf("Sent counter: %d, Received counter: %d\n", send_counter, received_counter);

        // sleep(1);
        send_counter++;
    }

    destroy_context(context);
}
