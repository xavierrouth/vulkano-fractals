#version 460

struct Parameters {
    dvec2 center;
    double time; 
    double scale;
    int iterations;
};

layout(local_size_x = 8, local_size_y = 8, local_size_z = 1) in;
layout(set = 0, binding = 0, rgba8) uniform writeonly image2D img;

layout(std140, binding = 1) readonly buffer ParametersIn {
    Parameters p;
};

void main() {
    // Coordinates scaled to the image size
    const double scale = p.scale; // .02;
    const dvec2 center = p.center;
    const dvec2 colorCenter = dvec2(0.0, 0.0);

    const dvec2 norm_coordinates = (gl_GlobalInvocationID.xy + dvec2(0.5)) / dvec2(imageSize(img));
    
    // orbit trap coloring
    double minDist = 1e20;
    double tempDist = 1e20;
    
    // How do we cast form float to double in glsl? 
    

    dvec2 c = (norm_coordinates - dvec2(0.5)) * scale + center;

    dvec2 z = dvec2(0.0, 0.0);

    const int maxIterations = p.iterations;

    int i;
    for (i = 0; i < maxIterations; i += 1) {
        z = dvec2(
            z.x * z.x - z.y * z.y + c.x, // real part
            z.y * z.x + z.x * z.y + c.y // imaginary part
        );

        tempDist = length(z - colorCenter);
        if (minDist > tempDist) {
            minDist = tempDist;
        }

        if (length(z) > 2.0) {
            break;
        }
    }

    double result = double(tempDist);
    vec4 to_write = vec4(1 - result, result, result, 1.0);

    imageStore(img, ivec2(gl_GlobalInvocationID.xy), to_write);
}