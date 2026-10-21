/* ImageKit.dll — image decode, transform, and encode behind one
 * clean C ABI. All strings UTF-8, NUL-terminated. char* results are
 * caller-owned: free with ik_free_string(). Byte buffers returned by
 * ik_encode_* are freed with ik_free_buffer(). Images live behind
 * uint64_t handles; every transform returns a NEW handle and never
 * mutates its source. Free handles with ik_free_image().
 * Formats in v0.1: PNG and BMP decode+encode, PNM (P5/P6) decode. */
#ifndef IMAGEKIT_H
#define IMAGEKIT_H
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
#if defined(_WIN32) && defined(IK_BUILD)
#define IK_API __declspec(dllexport)
#elif defined(_WIN32)
#define IK_API __declspec(dllimport)
#else
#define IK_API
#endif
#define IK_OK 0
#define IK_ERR_INVALID_ARG 1
#define IK_ERR_NOT_SUPPORTED 2
#define IK_ERR_NOT_FOUND 3
#define IK_ERR_ACCESS_DENIED 4
#define IK_ERR_PLATFORM 5
#define IK_ERR_INTERNAL 6
/* Extension beyond the shared 0-6 set: invalid image data. */
#define IK_ERR_CORRUPT 7

IK_API const char *ik_version(void);
IK_API int ik_version_major(void);
IK_API int ik_version_minor(void);
IK_API int ik_version_patch(void);
IK_API const char *ik_last_error(void);
IK_API int ik_last_error_code(void);
IK_API void ik_free_string(char *p);
IK_API void ik_free_buffer(uint8_t *p, size_t len);

/* Loading: format is sniffed from content (PNG/BMP/PNM). */
IK_API uint64_t ik_load_file(const char *path);
IK_API uint64_t ik_load_bytes(const uint8_t *data, size_t len);
IK_API int ik_free_image(uint64_t handle);
IK_API char *ik_info(uint64_t handle); /* {"width":W,"height":H,...} */
IK_API int ik_width(uint64_t handle);  /* -1 on error */
IK_API int ik_height(uint64_t handle); /* -1 on error */
IK_API int ik_get_pixel(uint64_t handle, uint32_t x, uint32_t y, uint32_t *out_rgba); /* 0xRRGGBBAA */

/* Transforms — each returns a new handle, or 0 on failure. */
IK_API uint64_t ik_resize(uint64_t handle, uint32_t new_w, uint32_t new_h, int bilinear);
IK_API uint64_t ik_thumbnail(uint64_t handle, uint32_t max_w, uint32_t max_h);
IK_API uint64_t ik_crop(uint64_t handle, uint32_t x, uint32_t y, uint32_t w, uint32_t h);
IK_API uint64_t ik_rotate(uint64_t handle, uint32_t degrees); /* 90, 180, 270 */
IK_API uint64_t ik_flip(uint64_t handle, int horizontal);     /* 0 = vertical */
IK_API uint64_t ik_grayscale(uint64_t handle);
IK_API uint64_t ik_invert(uint64_t handle);
IK_API uint64_t ik_adjust(uint64_t handle, int brightness, int contrast_pct);
IK_API uint64_t ik_blur(uint64_t handle, uint32_t radius);

/* Encoding. */
IK_API int ik_encode_png(uint64_t handle, uint8_t **out_ptr, size_t *out_len);
IK_API int ik_encode_bmp(uint64_t handle, uint8_t **out_ptr, size_t *out_len);
IK_API int ik_save_png(uint64_t handle, const char *path);
IK_API int ik_save_bmp(uint64_t handle, const char *path);

#ifdef __cplusplus
}
#endif
#endif
