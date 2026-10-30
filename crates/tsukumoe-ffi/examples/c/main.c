/* Copyright 2026 Tsukumoe Contributors
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 *
 * Loads a .tmod, moves the parameter "P" to 0, 0.5 and 1, and prints the
 * vertex positions of the draw list after each step.
 *
 *   tsukumoe-c-sample docs/specifications/samples/quad.tmod
 */
#define _CRT_SECURE_NO_WARNINGS
#include <stdio.h>
#include <stdlib.h>

#include "tsukumoe.h"

static void check(TsukumoeStatus status, const char *what) {
  if (status != TSUKUMOE_STATUS_OK) {
    fprintf(stderr, "%s failed (status %d): %s\n", what, (int)status,
            tsukumoe_last_error_message());
    exit(1);
  }
}

static unsigned char *read_file(const char *path, size_t *len) {
  FILE *f = fopen(path, "rb");
  unsigned char *data = NULL;
  long size;
  if (!f) {
    fprintf(stderr, "cannot open %s\n", path);
    exit(1);
  }
  fseek(f, 0, SEEK_END);
  size = ftell(f);
  fseek(f, 0, SEEK_SET);
  data = (unsigned char *)malloc(size > 0 ? (size_t)size : 1);
  if (!data || fread(data, 1, (size_t)size, f) != (size_t)size) {
    fprintf(stderr, "cannot read %s\n", path);
    exit(1);
  }
  fclose(f);
  *len = (size_t)size;
  return data;
}

int main(int argc, char **argv) {
  static const float values[] = {0.0f, 0.5f, 1.0f};
  size_t len = 0, step, v;
  unsigned char *data;
  TsukumoeRuntime *runtime = NULL;
  uint32_t p = 0, textures = 0, png_len = 0;
  const uint8_t *png = NULL;

  if (argc != 2) {
    fprintf(stderr, "usage: %s <file.tmod>\n", argv[0]);
    return 2;
  }
  data = read_file(argv[1], &len);

  /* The runtime copies the data, so the buffer can go right away. */
  check(tsukumoe_runtime_load(data, len, &runtime), "load");
  free(data);

  printf("abi %u.%u\n", (unsigned)(tsukumoe_abi_version() >> 16),
         (unsigned)(tsukumoe_abi_version() & 0xffff));
  check(tsukumoe_runtime_parameter_index(runtime, "P", &p), "parameter_index");

  for (step = 0; step < sizeof values / sizeof values[0]; step++) {
    TsukumoeDrawBuffers buffers;
    check(tsukumoe_runtime_set_parameter(runtime, p, values[step]),
          "set_parameter");
    check(tsukumoe_runtime_update(runtime, 0.0f), "update");
    check(tsukumoe_runtime_draw_buffers(runtime, &buffers), "draw_buffers");
    printf("P %g:", (double)values[step]);
    for (v = 0; v < (size_t)buffers.vertex_count * 2; v++) {
      printf(" %.9g", (double)buffers.positions[v]);
    }
    printf("\n");
  }

  check(tsukumoe_runtime_texture_count(runtime, &textures), "texture_count");
  if (textures > 0) {
    check(tsukumoe_runtime_texture(runtime, 0, &png, &png_len), "texture");
    printf("texture 0: %u bytes, %.3s\n", (unsigned)png_len, (const char *)png + 1);
  }

  check(tsukumoe_runtime_destroy(runtime), "destroy");
  return 0;
}
