extern crate nalgebra_glm as glm;

use std::env;
use std::sync::Arc;
use std::time::SystemTime;

use image::{ImageBuffer, Rgba};
use vulkano::device::physical::{PhysicalDevice, PhysicalDeviceType};
use vulkano::shader::{EntryPoint, ShaderModule};
use vulkano::{VulkanLibrary, Version, shader, Validated, VulkanError};
use vulkano::buffer::{Buffer, BufferCreateInfo, BufferUsage};
use vulkano::command_buffer::allocator::StandardCommandBufferAllocator;
use vulkano::command_buffer::{
    AutoCommandBufferBuilder, CommandBufferUsage, CopyImageToBufferInfo, BlitImageInfo,
};
use vulkano::descriptor_set::allocator::StandardDescriptorSetAllocator;
use vulkano::descriptor_set::{PersistentDescriptorSet, WriteDescriptorSet};
use vulkano::device::{Device, DeviceCreateInfo, QueueCreateInfo, QueueFlags, DeviceExtensions, Features, Queue};
use vulkano::format::Format;
use vulkano::image::view::ImageView;
use vulkano::image::{Image, ImageCreateInfo, ImageType, ImageUsage};
use vulkano::instance::{Instance, InstanceCreateInfo, InstanceCreateFlags};
use vulkano::memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator};
use vulkano::pipeline::compute::ComputePipelineCreateInfo;
use vulkano::pipeline::layout::PipelineDescriptorSetLayoutCreateInfo;
use vulkano::pipeline::{
    ComputePipeline, Pipeline, PipelineBindPoint, PipelineLayout, PipelineShaderStageCreateInfo,
};
use vulkano::swapchain::{Surface, Swapchain, SwapchainCreateInfo, PresentMode, SwapchainPresentInfo, acquire_next_image};
use vulkano::sync::{self, GpuFuture};
use winit::dpi::PhysicalPosition;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{EventLoop, ControlFlow};
use winit::window::{WindowBuilder, Window};

use crate::cs::Parameters;


// This has to be arrays actually, I don't feel like using bytemuck and glm::Vec2 for now. 

/* NOTE: This is actually autogenerated by shaders!(), use that instead */
/*
struct Parameters {
    center: [f32; 2], 
    time: f32,
    scale: f32,
}  */

pub fn create_swapchain(device: Arc<Device>, surface: &Arc<Surface>, window: &Arc<Window>) -> (Arc<Swapchain>, Vec<Arc<Image>>) {
    let (swapchain, images) = {
        // Querying the capabilities of the surface. When we create the swapchain we can only pass
        // values that are allowed by the capabilities.
        let surface_capabilities = device
            .physical_device()
            .surface_capabilities(&surface, Default::default())
            .unwrap();

        // Choosing the internal format that the images will have.
        let image_format = device
            .physical_device()
            .surface_formats(&surface, Default::default())
            .unwrap()[0]
            .0;

        // Please take a look at the docs for the meaning of the parameters we didn't mention.
        Swapchain::new(
            device.clone(),
            surface.clone(),
            SwapchainCreateInfo {
                // Some drivers report an `min_image_count` of 1, but fullscreen mode requires at
                // least 2. Therefore we must ensure the count is at least 2, otherwise the program
                // would crash when entering fullscreen mode on those drivers.
                min_image_count: surface_capabilities.min_image_count.max(2),
                //pre_transform: SurfaceTransform::HorizontalMirror,
                present_mode: PresentMode::Fifo,
                

                image_format,

                // The size of the window, only used to initially setup the swapchain.
                //
                // NOTE:
                // On some drivers the swapchain extent is specified by
                // `surface_capabilities.current_extent` and the swapchain size must use this
                // extent. This extent is always the same as the window size.
                //
                // However, other drivers don't specify a value, i.e.
                // `surface_capabilities.current_extent` is `None`. These drivers will allow
                // anything, but the only sensible value is the window size.
                //
                // Both of these cases need the swapchain to use the window size, so we just
                // use that.
                image_extent: window.inner_size().into(),

                image_usage: ImageUsage::COLOR_ATTACHMENT | ImageUsage::TRANSFER_DST,

                // The alpha mode indicates how the alpha value of the final image will behave. For
                // example, you can choose whether the window will be opaque or transparent.
                composite_alpha: surface_capabilities
                    .supported_composite_alpha
                    .into_iter()
                    .next()
                    .unwrap(),

                ..Default::default()
            },
        )
        .unwrap()
    };

    (swapchain, images)
}

mod cs {
    vulkano_shaders::shader! {
        ty: "compute",
        path: "src/fractal.glsl",
    }
}

pub fn create_compute_pipeline(device: Arc<Device>) -> Arc<ComputePipeline> {
    
    let shader: Arc<ShaderModule> = cs::load(device.clone()).expect("failed to create shader module");
    
    let entry_point: EntryPoint = shader.entry_point("main").unwrap();

    let stage = PipelineShaderStageCreateInfo::new(entry_point);

    let layout = PipelineLayout::new(
        device.clone(),
        PipelineDescriptorSetLayoutCreateInfo::from_stages([&stage])
            .into_pipeline_layout_create_info(device.clone())
            .unwrap(),
    )
    .unwrap();

    let compute_pipeline = ComputePipeline::new(
        device.clone(),
        None,
        ComputePipelineCreateInfo::stage_layout(stage, layout),
    )
    .expect("failed to create compute pipeline");

    compute_pipeline
}

pub fn select_device(
    instance: Arc<Instance>, 
    mut device_extensions: DeviceExtensions, 
    surface: &Arc<Surface>
) 
    -> (Arc<Device>, Arc<Queue>) 
    {
    let (physical_device, queue_family_index) = instance
        .enumerate_physical_devices()
        .unwrap()
        .filter(|p| {
            // For this example, we require at least Vulkan 1.3, or a device that has the
            // `khr_dynamic_rendering` extension available.
            p.api_version() >= Version::V1_3 || p.supported_extensions().khr_dynamic_rendering
        })
        .filter(|p| {
            // Some devices may not support the extensions or features that your application, or
            // report properties and limits that are not sufficient for your application. These
            // should be filtered out here.
            p.supported_extensions().contains(&device_extensions)
        })
        .filter_map(|p| {
            // For each physical device, we try to find a suitable queue family that will execute
            // our draw commands.
            //
            // Devices can provide multiple queues to run commands in parallel (for example a draw
            // queue and a compute queue), similar to CPU threads. This is something you have to
            // have to manage manually in Vulkan. Queues of the same type belong to the same queue
            // family.
            //
            // Here, we look for a single queue family that is suitable for our purposes. In a
            // real-world application, you may want to use a separate dedicated transfer queue to
            // handle data transfers in parallel with graphics operations. You may also need a
            // separate queue for compute operations, if your application uses those.
            p.queue_family_properties()
                .iter()
                .enumerate()
                .position(|(i, q)| {
                    // We select a queue family that supports graphics operations. When drawing to
                    // a window surface, as we do in this example, we also need to check that
                    // queues in this queue family are capable of presenting images to the surface.
                    q.queue_flags.intersects(QueueFlags::COMPUTE)
                        && p.surface_support(i as u32, &surface).unwrap_or(false)
                })
                // The code here searches for the first queue family that is suitable. If none is
                // found, `None` is returned to `filter_map`, which disqualifies this physical
                // device.
                .map(|i| (p, i as u32))
        })
        // All the physical devices that pass the filters above are suitable for the application.
        // However, not every device is equal, some are preferred over others. Now, we assign each
        // physical device a score, and pick the device with the lowest ("best") score.
        //
        // In this example, we simply select the best-scoring device to use in the application.
        // In a real-world setting, you may want to use the best-scoring device only as a "default"
        // or "recommended" device, and let the user choose the device themself.
        .min_by_key(|(p, _)| {
            // We assign a lower score to device types that are likely to be faster/better.
            match p.properties().device_type {
                PhysicalDeviceType::DiscreteGpu => 0,
                PhysicalDeviceType::IntegratedGpu => 1,
                PhysicalDeviceType::VirtualGpu => 2,
                PhysicalDeviceType::Cpu => 3,
                PhysicalDeviceType::Other => 4,
                _ => 5,
            }
        })
    .expect("no suitable physical device found");

    if physical_device.api_version() < Version::V1_3 {
        device_extensions.khr_dynamic_rendering = true;
    }

    // Some little debug infos.
    println!(
        "Using device: {} (type: {:?})",
        physical_device.properties().device_name,
        physical_device.properties().device_type,
    );

    // If the selected device doesn't have Vulkan 1.3 available, then we need to enable the
    // `khr_dynamic_rendering` extension manually. This extension became a core part of Vulkan
    // in version 1.3 and later, so it's always available then and it does not need to be enabled.
    // We can be sure that this extension will be available on the selected physical device,
    // because we filtered out unsuitable devices in the device selection code above.
    if physical_device.api_version() < Version::V1_3 {
        device_extensions.khr_dynamic_rendering = true;
    }

    let (device, mut queues) = Device::new(
        // Which physical device to connect to.
        physical_device,
        DeviceCreateInfo {
            // The list of queues that we are going to use. Here we only use one queue, from the
            // previously chosen queue family.
            queue_create_infos: vec![QueueCreateInfo {
                queue_family_index,
                ..Default::default()
            }],

            // A list of optional features and extensions that our program needs to work correctly.
            // Some parts of the Vulkan specs are optional and must be enabled manually at device
            // creation. In this example the only things we are going to need are the
            // `khr_swapchain` extension that allows us to draw to a window, and
            // `khr_dynamic_rendering` if we don't have Vulkan 1.3 available.
            enabled_extensions: device_extensions,

            // In order to render with Vulkan 1.3's dynamic rendering, we need to enable it here.
            // Otherwise, we are only allowed to render with a render pass object, as in the
            // standard triangle example. The feature is required to be supported by the device if
            // it supports Vulkan 1.3 and higher, or if the `khr_dynamic_rendering` extension is
            // available, so we don't need to check for support.
            enabled_features: Features {
                shader_float64: true,
                dynamic_rendering: true,
                ..Features::empty()
            },

            ..Default::default()
        },
    )
    .unwrap();

    let queue = queues.next().unwrap();
    (device, queue)
}

fn main() {
    println!("Hello, world!");
    env::set_var("RUST_BACKTRACE", "1");
    
    let event_loop = EventLoop::new();


    let library = VulkanLibrary::new().unwrap();
    /* Get extensions required by display */
    let required_extensions = Surface::required_extensions(&event_loop);

    let instance = Instance::new(
        library,
        InstanceCreateInfo {
            //flags: InstanceCreateFlags::ENUMERATE_PORTABILITY,
            enabled_extensions: required_extensions,
            ..Default::default()
        }
    )
    .unwrap();

    /* Set up window (winit) & surface (vulkan) */
    let window: Arc<Window> = Arc::new(WindowBuilder::new().build(&event_loop).unwrap());
    let surface: Arc<Surface> = Surface::from_window(instance.clone(), window.clone()).unwrap();

    let device_extensions = DeviceExtensions {
        khr_swapchain: true,
        ..DeviceExtensions::empty()
    };

    let (device, queue) = select_device(instance, device_extensions, &surface);

    let (mut swapchain, mut swapchain_images) = create_swapchain(device.clone(), &surface, &window);

    let memory_allocator = Arc::new(StandardMemoryAllocator::new_default(device.clone()));

    let compute_pipeline = create_compute_pipeline(device.clone());

    /* Make an image to put the fractal on */
    // TODO: Don't we need a new image for each frame in the swapchain?
    let fractal_image = Image::new(
        memory_allocator.clone(),
        ImageCreateInfo {
            image_type: ImageType::Dim2d,
            format: Format::R8G8B8A8_UNORM,
            extent: [1024, 1024, 1],
            usage: ImageUsage::STORAGE | ImageUsage::TRANSFER_SRC,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
            ..Default::default()
        },
    )
    .unwrap();

    
    let descriptor_set_allocator =
        StandardDescriptorSetAllocator::new(device.clone(), Default::default());



    let command_buffer_allocator =
        StandardCommandBufferAllocator::new(device.clone(), Default::default());


    let mut recreate_swapchain = false;

    // In the loop below we are going to submit commands to the GPU. Submitting a command produces
    // an object that implements the `GpuFuture` trait, which holds the resources for as long as
    // they are in use by the GPU.
    //
    // Destroying the `GpuFuture` blocks until the GPU is finished executing it. In order to avoid
    // that, we store the submission of the previous frame here.
    let mut previous_frame_end = Some(sync::now(device.clone()).boxed());
    let start = SystemTime::now();

    let mut mouse_pos: PhysicalPosition<f64> = PhysicalPosition::default();
    
    event_loop.run(move |event, _, control_flow| {
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(_),
                ..
            } => {
                recreate_swapchain = true;
            }
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { device_id, position, modifiers },
                ..
            } => {
                mouse_pos = position;
            }
            Event::RedrawEventsCleared => {
                
                // Do not draw the frame when the screen size is zero. On Windows, this can
                // occur when minimizing the application.
                let image_extent: [u32; 2] = window.inner_size().into();

                if image_extent.contains(&0) {
                    return;
                }

                // It is important to call this function from time to time, otherwise resources
                // will keep accumulating and you will eventually reach an out of memory error.
                // Calling this function polls various fences in order to determine what the GPU
                // has already processed, and frees the resources that are no longer needed.
                previous_frame_end.as_mut().unwrap().cleanup_finished();

                // Whenever the window resizes we need to recreate everything dependent on the
                // window size. In this example that includes the swapchain, the framebuffers and
                // the dynamic state viewport.
                if recreate_swapchain {
                    let (new_swapchain, new_images) = swapchain
                        .recreate(SwapchainCreateInfo {
                            image_extent,
                            ..swapchain.create_info()
                        })
                        .expect("failed to recreate swapchain");

                    swapchain = new_swapchain;

                    // Now that we have new swapchain images, we must create new image views from
                    // them as well.

                    // How about no... 
                    swapchain_images = new_images;

                    recreate_swapchain = false;
                }

                // Before we can draw on the output, we have to *acquire* an image from the
                // swapchain. If no image is available (which happens if you submit draw commands
                // too quickly), then the function will block. This operation returns the index of
                // the image that we are allowed to draw upon.
                //
                // This function can block if no image is available. The parameter is an optional
                // timeout after which the function call will return an error.
                let (image_index, suboptimal, acquire_future) =
                    match acquire_next_image(swapchain.clone(), None).map_err(Validated::unwrap) {
                        Ok(r) => r,
                        Err(VulkanError::OutOfDate) => {
                            recreate_swapchain = true;
                            return;
                        }
                        Err(e) => panic!("failed to acquire next image: {e}"),
                    };

                // `acquire_next_image` can be successful, but suboptimal. This means that the
                // swapchain image will still work, but it may not display correctly. With some
                // drivers this can be when the window resizes, but it may not cause the swapchain
                // to become out of date.
                if suboptimal {
                    recreate_swapchain = true;
                }
                
                let zoomtime = 70.0;

                let mut time: f64 = SystemTime::now().duration_since(start)
                    .unwrap().as_secs_f64() / 20.0;
                
                
                //let scale = 1.0 / ((((time as f64 * (time as f64 * 0.001)) as i128 % 1000000000) / 10000 )) as f32;

                let mut zoom = 0.7 + 0.38 * (1.2 * time.cos());

                let iterations = 100 + ((12.0 / zoom.powi(2)) as u32) % 800;

                zoom = zoom.powi(8);

                
                let mut x_pos = mouse_pos.x as f64 / window.inner_size().width as f64;
                let mut y_pos = mouse_pos.y as f64 / window.inner_size().height as f64;

                x_pos = (x_pos - 0.5) * 3.0;
                y_pos = (y_pos - 0.5) * 3.0;

                //println!("x: {}, y: {}", mouse_pos.x, mouse_pos.y);


                /* Parameters Buffer */
                /* Create a storage buffer (What are push constants, should we use those instead? ) */

                /* Julia Set: */
                x_pos = -0.162;
                y_pos = -1.04;

                let parameters = cs::Parameters {
                    center: [0.0, 0.0], //[-0.7451544, 0.1853],
                    time: 0.0,
                    scale: 0.5, // zoom as f64, // time * 100.0,
                    mouse_pos: [x_pos, y_pos],
                    iterations: 300 as i32, 
                    // ((iterations % 10000) / 100 ) as i32,
                };

                //println!("xpos: {x_pos} ypos:{y_pos}");
                /* Mandelbrot */
                
                let _parameters = cs::Parameters {
                    center: [-0.7451544, 0.1853],
                    time: 0.0,
                    scale: zoom as f64, 
                    mouse_pos: [0.0, 0.0],
                    iterations: iterations as i32, 
                    // ((iterations % 10000) / 100 ) as i32,
                };  

                // TODO: Reuuse buffer, or make it a staging buffer
                let parameters_buffer = Buffer::from_data(
                    memory_allocator.clone(),
                    BufferCreateInfo {
                        usage: BufferUsage::STORAGE_BUFFER,
                        ..Default::default()
                    },
                    AllocationCreateInfo {
                        memory_type_filter: 
                        MemoryTypeFilter::PREFER_DEVICE |
                        MemoryTypeFilter::HOST_RANDOM_ACCESS,
                        ..Default::default()
                    },
                    parameters
                )
                .expect("failed to create buffer");

               
                

                /* Attach to compute pipeline */
                let view = ImageView::new_default(fractal_image.clone()).unwrap();

                let count = compute_pipeline.layout().set_layouts().len();

                //println!("count: {count}");

                let layout = compute_pipeline.layout().set_layouts().get(0).unwrap();
                    
                let set: Arc<PersistentDescriptorSet> = PersistentDescriptorSet::new(
                    &descriptor_set_allocator,
                    layout.clone(),
                    [WriteDescriptorSet::image_view(0, view), 
                    WriteDescriptorSet::buffer(1, parameters_buffer)
                    ], // 0 is the binding
                    [],
                )
                .expect("Invalid descriptor set");

                    
                // In order to draw, we have to build a *command buffer*. The command buffer object
                // holds the list of commands that are going to be executed.
                //
                // Building a command buffer is an expensive operation (usually a few hundred
                // microseconds), but it is known to be a hot path in the driver and is expected to
                // be optimized.
                //
                // Note that we have to pass a queue family when we create the command buffer. The
                // command buffer will only be executable on that given queue family.
                let mut builder = AutoCommandBufferBuilder::primary(
                    &command_buffer_allocator,
                    queue.queue_family_index(),
                    CommandBufferUsage::OneTimeSubmit,
                )
                .unwrap();
                
                // TODO: Make this use a compute queue, not a graphics queue. 
                builder
                    .bind_pipeline_compute(compute_pipeline.clone())
                    .unwrap()
                    .bind_descriptor_sets(
                        PipelineBindPoint::Compute,
                        compute_pipeline.layout().clone(),
                        0,
                        set,
                    )
                    .unwrap()
                    .dispatch([1024 / 16, 1024 / 16, 1])
                    .unwrap()
                    .blit_image(
                        BlitImageInfo::images(fractal_image.clone(), swapchain_images[image_index as usize].clone())
                    )
                    .unwrap();
                    

                // Finish building the command buffer by calling `build`.
                let command_buffer = builder.build().unwrap();

                let future = previous_frame_end
                    .take()
                    .unwrap()
                    .join(acquire_future)
                    .then_execute(queue.clone(), command_buffer)
                    .unwrap()
                    // The color output is now expected to contain our triangle. But in order to
                    // show it on the screen, we have to *present* the image by calling
                    // `then_swapchain_present`.
                    //
                    // This function does not actually present the image immediately. Instead it
                    // submits a present command at the end of the queue. This means that it will
                    // only be presented once the GPU has finished executing the command buffer
                    // that draws the triangle.
                    .then_swapchain_present(
                        queue.clone(),
                        SwapchainPresentInfo::swapchain_image_index(swapchain.clone(), image_index),
                    )
                    .then_signal_fence_and_flush();

                match future.map_err(Validated::unwrap) {
                    Ok(future) => {
                        previous_frame_end = Some(future.boxed());
                    }
                    Err(VulkanError::OutOfDate) => {
                        recreate_swapchain = true;
                        previous_frame_end = Some(sync::now(device.clone()).boxed());
                    }
                    Err(e) => {
                        println!("failed to flush future: {e}");
                        previous_frame_end = Some(sync::now(device.clone()).boxed());
                    }
                }
            }
        _ => (),
        }
    });


    //let buffer_content = buf.read().unwrap();
    //let image = ImageBuffer::<Rgba<u8>, _>::from_raw(1024, 1024, &buffer_content[..]).unwrap();
    //image.save("image.png").unwrap();

    println!("Everything succeeded!");

}
