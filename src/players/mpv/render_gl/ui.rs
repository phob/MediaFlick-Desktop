//! One reusable premultiplied BGRA texture, composited after video rendering.
use crate::playback::NativeOverlayFrame;
use glow::HasContext;
use std::io;
use std::ops::Range;

pub(super) struct Ui {
    program: glow::NativeProgram,
    vao: glow::NativeVertexArray,
    texture: glow::NativeTexture,
    size: (u16, u16),
    bands: [Option<Range<usize>>; 2],
}
impl Ui {
    pub fn new(gl: &glow::Context) -> io::Result<Self> {
        let program = program(gl)?;
        let vao = match unsafe { gl.create_vertex_array() } {
            Ok(vao) => vao,
            Err(error) => {
                unsafe {
                    gl.delete_program(program);
                }
                return Err(io::Error::other(error));
            }
        };
        let texture = match unsafe { gl.create_texture() } {
            Ok(texture) => texture,
            Err(error) => {
                unsafe {
                    gl.delete_program(program);
                    gl.delete_vertex_array(vao);
                }
                return Err(io::Error::other(error));
            }
        };
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            for name in [glow::TEXTURE_MIN_FILTER, glow::TEXTURE_MAG_FILTER] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, name, glow::LINEAR as i32);
            }
            for name in [glow::TEXTURE_WRAP_S, glow::TEXTURE_WRAP_T] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, name, glow::CLAMP_TO_EDGE as i32);
            }
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
        Ok(Self {
            program,
            vao,
            texture,
            size: (0, 0),
            bands: [None, None],
        })
    }
    pub fn upload(&mut self, gl: &glow::Context, frame: &NativeOverlayFrame) {
        self.bands = visible_bands(frame);
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
            if self.size != (frame.width, frame.height) {
                gl.finish();
                gl.tex_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    glow::RGBA8 as i32,
                    i32::from(frame.width),
                    i32::from(frame.height),
                    0,
                    glow::BGRA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&frame.pixels)),
                );
                self.size = (frame.width, frame.height);
            } else {
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    i32::from(frame.width),
                    i32::from(frame.height),
                    glow::BGRA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&frame.pixels)),
                );
            }
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }
    pub fn draw(&self, gl: &glow::Context, width: i32, height: i32) {
        if self.size == (0, 0) {
            return;
        }
        unsafe {
            gl.viewport(0, 0, width, height);
            gl.use_program(Some(self.program));
            gl.bind_vertex_array(Some(self.vao));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.enable(glow::SCISSOR_TEST);
            for rows in self.bands.iter().flatten() {
                let top = display_row(rows.start, self.size.1, height);
                let bottom = display_row(rows.end, self.size.1, height);
                gl.scissor(0, height - bottom, width, bottom - top);
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
            }
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_vertex_array(None);
            gl.use_program(None);
        }
    }
    pub fn destroy(self, gl: &glow::Context) {
        unsafe {
            gl.delete_texture(self.texture);
            gl.delete_vertex_array(self.vao);
            gl.delete_program(self.program);
        }
    }
}
fn program(gl: &glow::Context) -> io::Result<glow::NativeProgram> {
    let program = unsafe { gl.create_program() }.map_err(io::Error::other)?;
    let result = link(gl, program);
    if let Err(error) = result {
        unsafe {
            gl.delete_program(program);
        }
        return Err(error);
    }
    Ok(program)
}
fn link(gl: &glow::Context, program: glow::NativeProgram) -> io::Result<()> {
    for (kind, source) in [
        (
            glow::VERTEX_SHADER,
            "#version 330 core\nout vec2 uv;\nvoid main(){ vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2); uv=vec2(p.x,1.0-p.y); gl_Position=vec4(p*2.0-1.0,0.0,1.0); }",
        ),
        (
            glow::FRAGMENT_SHADER,
            "#version 330 core\nin vec2 uv;\nout vec4 color;\nuniform sampler2D bitmap;\nvoid main(){ color=texture(bitmap,uv); }",
        ),
    ] {
        let shader = unsafe { gl.create_shader(kind) }.map_err(io::Error::other)?;
        unsafe {
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                let error = gl.get_shader_info_log(shader);
                gl.delete_shader(shader);
                return Err(io::Error::other(error));
            }
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
    }
    unsafe {
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(io::Error::other(gl.get_program_info_log(program)));
        }
    }
    Ok(())
}

fn visible_bands(frame: &NativeOverlayFrame) -> [Option<Range<usize>>; 2] {
    let stride = usize::from(frame.width) * 4;
    let height = usize::from(frame.height);
    let transparent = vec![0; stride];
    let split = height.div_ceil(2);
    [0..split, split..height].map(|band| {
        let pixels = &frame.pixels[band.start * stride..band.end * stride];
        let first = pixels
            .chunks_exact(stride)
            .position(|row| row != transparent)?;
        let last = pixels
            .chunks_exact(stride)
            .rposition(|row| row != transparent)?;
        Some(band.start + first..band.start + last + 1)
    })
}
fn display_row(row: usize, native_height: u16, drawable_height: i32) -> i32 {
    ((row as i64 * i64::from(drawable_height) + i64::from(native_height) / 2)
        / i64::from(native_height)) as i32
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hidden_controls_skip_transparent_pixels_and_menus_preserve_all_rows() {
        let mut frame = NativeOverlayFrame {
            width: 4,
            height: 3,
            pixels: vec![0; 48],
        };
        assert_eq!(visible_bands(&frame), [None, None]);
        frame.pixels[3] = 255;
        frame.pixels[47] = 255;
        assert_eq!(visible_bands(&frame), [Some(0..1), Some(2..3)]);
        frame.pixels.fill(255);
        assert_eq!(visible_bands(&frame), [Some(0..2), Some(2..3)]);
        assert_eq!(display_row(2, 3, 5), 3);
        assert_eq!(display_row(3, 3, 5), 5);
    }
}
