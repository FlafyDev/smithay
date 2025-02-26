//! Helper for effective damage tracked rendering
//!
//! # Why use this implementation
//!
//! The [`DamageTrackedRenderer`] in combination with the [`RenderElement`] trait
//! can help you to reduce resource consumption by tracking what elements have
//! been damaged and only redraw the damaged parts on an output.
//!
//! It does so by keeping track of the last used [`CommitCounter`] for all provided
//! [`RenderElement`]s and queries the element for new damage on each call to [`render_output`](DamageTrackedRenderer::render_output).
//!
//! You can initialize it with a static output by using [`DamageTrackedRenderer::new`] or
//! allow it to track a specific [`Output`] with [`DamageTrackedRenderer::from_output`].
//!
//! See the [`renderer::element`](crate::backend::renderer::element) module for more information
//! about how to use [`RenderElement`].
//!
//! # How to use it
//!
//! ```no_run
//! # use smithay::{
//! #     backend::renderer::{Frame, ImportMem, Renderer, Texture, TextureFilter},
//! #     utils::{Buffer, Physical, Rectangle, Size},
//! # };
//! # use slog::Drain;
//! #
//! # #[derive(Clone)]
//! # struct FakeTexture;
//! #
//! # impl Texture for FakeTexture {
//! #     fn width(&self) -> u32 {
//! #         unimplemented!()
//! #     }
//! #     fn height(&self) -> u32 {
//! #         unimplemented!()
//! #     }
//! # }
//! #
//! # struct FakeFrame;
//! #
//! # impl Frame for FakeFrame {
//! #     type Error = std::convert::Infallible;
//! #     type TextureId = FakeTexture;
//! #
//! #     fn id(&self) -> usize { unimplemented!() }
//! #     fn clear(&mut self, _: [f32; 4], _: &[Rectangle<i32, Physical>]) -> Result<(), Self::Error> {
//! #         unimplemented!()
//! #     }
//! #     fn render_texture_from_to(
//! #         &mut self,
//! #         _: &Self::TextureId,
//! #         _: Rectangle<f64, Buffer>,
//! #         _: Rectangle<i32, Physical>,
//! #         _: &[Rectangle<i32, Physical>],
//! #         _: Transform,
//! #         _: f32,
//! #     ) -> Result<(), Self::Error> {
//! #         unimplemented!()
//! #     }
//! #     fn transformation(&self) -> Transform {
//! #         unimplemented!()
//! #     }
//! #     fn finish(self) -> Result<(), Self::Error> { unimplemented!() }
//! # }
//! #
//! # struct FakeRenderer;
//! #
//! # impl Renderer for FakeRenderer {
//! #     type Error = std::convert::Infallible;
//! #     type TextureId = FakeTexture;
//! #     type Frame<'a> = FakeFrame;
//! #
//! #     fn id(&self) -> usize {
//! #         unimplemented!()
//! #     }
//! #     fn downscale_filter(&mut self, _: TextureFilter) -> Result<(), Self::Error> {
//! #         unimplemented!()
//! #     }
//! #     fn upscale_filter(&mut self, _: TextureFilter) -> Result<(), Self::Error> {
//! #         unimplemented!()
//! #     }
//! #     fn render(&mut self, _: Size<i32, Physical>, _: Transform) -> Result<Self::Frame<'_>, Self::Error>
//! #     {
//! #         unimplemented!()
//! #     }
//! # }
//! #
//! # impl ImportMem for FakeRenderer {
//! #     fn import_memory(
//! #         &mut self,
//! #         _: &[u8],
//! #         _: Size<i32, Buffer>,
//! #         _: bool,
//! #     ) -> Result<Self::TextureId, Self::Error> {
//! #         unimplemented!()
//! #     }
//! #     fn update_memory(
//! #         &mut self,
//! #         _: &Self::TextureId,
//! #         _: &[u8],
//! #         _: Rectangle<i32, Buffer>,
//! #     ) -> Result<(), Self::Error> {
//! #         unimplemented!()
//! #     }
//! # }
//! use smithay::{
//!     backend::renderer::{
//!         damage::DamageTrackedRenderer,
//!         element::memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement}
//!     },
//!     utils::{Point, Transform},
//! };
//! use std::time::{Duration, Instant};
//!
//! const WIDTH: i32 = 10;
//! const HEIGHT: i32 = 10;
//! # let mut renderer = FakeRenderer;
//! # let buffer_age = 0;
//! # let log = slog::Logger::root(slog::Discard.fuse(), slog::o!());
//!
//! // Initialize a new damage tracked renderer
//! let mut damage_tracked_renderer = DamageTrackedRenderer::new((800, 600), 1.0, Transform::Normal);
//!
//! // Initialize a buffer to render
//! let mut memory_buffer = MemoryRenderBuffer::new((WIDTH, HEIGHT), 1, Transform::Normal, None);
//!
//! let mut last_update = Instant::now();
//!
//! loop {
//!     let now = Instant::now();
//!     if now.duration_since(last_update) >= Duration::from_secs(3) {
//!         let mut render_context = memory_buffer.render();
//!
//!         render_context.draw(|_buffer| {
//!             // Update the changed parts of the buffer
//!
//!             // Return the updated parts
//!             Result::<_, ()>::Ok(vec![Rectangle::from_loc_and_size(Point::default(), (WIDTH, HEIGHT))])
//!         });
//!
//!         last_update = now;
//!     }
//!
//!     // Create a render element from the buffer
//!     let location = Point::from((100.0, 100.0));
//!     let render_element =
//!         MemoryRenderBufferRenderElement::from_buffer(&mut renderer, location, &memory_buffer, None, None, None, None)
//!         .expect("Failed to upload memory to gpu");
//!
//!     // Render the output
//!     damage_tracked_renderer
//!         .render_output(
//!             &mut renderer,
//!             buffer_age,
//!             &[render_element],
//!             [0.8, 0.8, 0.9, 1.0],
//!             log.clone(),
//!         )
//!         .expect("failed to render the output");
//! }
//! ```

use std::collections::{HashMap, VecDeque};

use indexmap::IndexMap;

use crate::{
    backend::renderer::{element::RenderElementPresentationState, Frame},
    output::Output,
    utils::{Physical, Rectangle, Scale, Size, Transform},
};

use super::{
    element::{Element, Id, RenderElement, RenderElementState, RenderElementStates},
    utils::CommitCounter,
};

use super::{Renderer, Texture};

#[derive(Debug, Clone, Copy)]
struct ElementInstanceState {
    last_geometry: Rectangle<i32, Physical>,
    last_z_index: usize,
}

impl ElementInstanceState {
    fn matches(&self, geometry: Rectangle<i32, Physical>, z_index: usize) -> bool {
        self.last_geometry == geometry && self.last_z_index == z_index
    }
}

#[derive(Debug, Clone)]
struct ElementState {
    last_commit: CommitCounter,
    last_instances: Vec<ElementInstanceState>,
}

impl ElementState {
    fn instance_matches(&self, geometry: Rectangle<i32, Physical>, z_index: usize) -> bool {
        self.last_instances
            .iter()
            .any(|instance| instance.matches(geometry, z_index))
    }
}

#[derive(Debug, Default)]
struct RendererState {
    size: Option<Size<i32, Physical>>,
    elements: IndexMap<Id, ElementState>,
    old_damage: VecDeque<Vec<Rectangle<i32, Physical>>>,
}

/// Mode for the [`DamageTrackedRenderer`]
#[derive(Debug, Clone)]
pub enum DamageTrackedRendererMode {
    /// Automatic mode based on a output
    Auto(Output),
    /// Static mode
    Static {
        /// Size of the static output
        size: Size<i32, Physical>,
        /// Scale of the static output
        scale: Scale<f64>,
        /// Transform of the static output
        transform: Transform,
    },
}

/// Output has no active mode
#[derive(Debug, thiserror::Error)]
#[error("Output has no active mode")]
pub struct OutputNoMode;

impl TryInto<(Size<i32, Physical>, Scale<f64>, Transform)> for DamageTrackedRendererMode {
    type Error = OutputNoMode;

    fn try_into(self) -> Result<(Size<i32, Physical>, Scale<f64>, Transform), Self::Error> {
        match self {
            DamageTrackedRendererMode::Auto(output) => Ok((
                output.current_mode().ok_or(OutputNoMode)?.size,
                output.current_scale().fractional_scale().into(),
                output.current_transform(),
            )),
            DamageTrackedRendererMode::Static {
                size,
                scale,
                transform,
            } => Ok((size, scale, transform)),
        }
    }
}

/// Damage tracked renderer for a single output
#[derive(Debug)]
pub struct DamageTrackedRenderer {
    mode: DamageTrackedRendererMode,
    last_state: RendererState,
}

/// Errors thrown by [`DamageTrackedRenderer::render_output`]
#[derive(thiserror::Error)]
pub enum DamageTrackedRendererError<R: Renderer> {
    /// The provided [`Renderer`] returned an error
    #[error(transparent)]
    Rendering(R::Error),
    /// The given [`Output`] has no mode set
    #[error(transparent)]
    OutputNoMode(#[from] OutputNoMode),
}

impl<R: Renderer> std::fmt::Debug for DamageTrackedRendererError<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DamageTrackedRendererError::Rendering(err) => std::fmt::Debug::fmt(err, f),
            DamageTrackedRendererError::OutputNoMode(err) => std::fmt::Debug::fmt(err, f),
        }
    }
}

impl DamageTrackedRenderer {
    /// Initialize a static [`DamageTrackedRenderer`]
    pub fn new(
        size: impl Into<Size<i32, Physical>>,
        scale: impl Into<Scale<f64>>,
        transform: Transform,
    ) -> Self {
        Self {
            mode: DamageTrackedRendererMode::Static {
                size: size.into(),
                scale: scale.into(),
                transform,
            },
            last_state: Default::default(),
        }
    }

    /// Initialize a new [`DamageTrackedRenderer`] from an [`Output`]
    ///
    /// The renderer will keep track of changes to the [`Output`]
    /// and handle size and scaling changes automatically on the
    /// next call to [`render_output`](DamageTrackedRenderer::render_output)
    pub fn from_output(output: &Output) -> Self {
        Self {
            mode: DamageTrackedRendererMode::Auto(output.clone()),
            last_state: Default::default(),
        }
    }

    /// Get the [`DamageTrackedRendererMode`] of the [`DamageTrackedRenderer`]
    pub fn mode(&self) -> &DamageTrackedRendererMode {
        &self.mode
    }

    /// Render this output
    pub fn render_output<E, R>(
        &mut self,
        renderer: &mut R,
        age: usize,
        elements: &[E],
        clear_color: [f32; 4],
        log: impl Into<Option<slog::Logger>>,
    ) -> Result<(Option<Vec<Rectangle<i32, Physical>>>, RenderElementStates), DamageTrackedRendererError<R>>
    where
        E: RenderElement<R>,
        R: Renderer,
        <R as Renderer>::TextureId: Texture,
    {
        let log = crate::slog_or_fallback(log);

        let (output_size, output_scale, output_transform) = self.mode.clone().try_into()?;
        // We have to apply to output transform to the output size so that the intersection
        // tests in damage_output_internal produces the correct results and do not crop
        // damage with the wrong size
        let output_geo = Rectangle::from_loc_and_size((0, 0), output_transform.transform_size(output_size));

        // This will hold all the damage we need for this rendering step
        let mut damage: Vec<Rectangle<i32, Physical>> = Vec::new();
        let mut render_elements: Vec<&E> = Vec::with_capacity(elements.len());
        let mut opaque_regions: Vec<(usize, Vec<Rectangle<i32, Physical>>)> = Vec::new();
        let states = self.damage_output_internal(
            age,
            elements,
            &log,
            output_scale,
            output_geo,
            &mut damage,
            &mut render_elements,
            &mut opaque_regions,
        );

        if damage.is_empty() {
            slog::trace!(log, "no damage, skipping rendering");
            return Ok((None, states));
        }

        slog::trace!(
            log,
            "rendering with damage {:?} and opaque regions {:?}",
            damage,
            opaque_regions
        );

        let render_res = (|| {
            let mut frame = renderer.render(output_size, output_transform)?;

            let clear_damage = opaque_regions.iter().flat_map(|(_, regions)| regions).fold(
                damage.clone(),
                |damage, region| {
                    damage
                        .into_iter()
                        .flat_map(|geo| geo.subtract_rect(*region))
                        .collect::<Vec<_>>()
                },
            );

            slog::trace!(log, "clearing damage {:?}", clear_damage);
            frame.clear(clear_color, &clear_damage)?;

            for (mut z_index, element) in render_elements.iter().rev().enumerate() {
                // This is necessary because we reversed the render elements to draw
                // them back to front, but z-index including opaque regions is defined
                // front to back
                z_index = render_elements.len() - 1 - z_index;

                let element_id = element.id();
                let element_geometry = element.geometry(output_scale);

                let element_damage = opaque_regions
                    .iter()
                    .filter(|(index, _)| *index < z_index)
                    .flat_map(|(_, regions)| regions)
                    .fold(
                        damage
                            .clone()
                            .into_iter()
                            .filter_map(|d| d.intersection(element_geometry))
                            .collect::<Vec<_>>(),
                        |damage, region| {
                            damage
                                .into_iter()
                                .flat_map(|geo| geo.subtract_rect(*region))
                                .collect::<Vec<_>>()
                        },
                    )
                    .into_iter()
                    .map(|mut d| {
                        d.loc -= element_geometry.loc;
                        d
                    })
                    .collect::<Vec<_>>();

                if element_damage.is_empty() {
                    slog::trace!(
                        log,
                        "skipping rendering element {:?} with geometry {:?}, no damage",
                        element_id,
                        element_geometry
                    );
                    continue;
                }

                slog::trace!(
                    log,
                    "rendering element {:?} with geometry {:?} and damage {:?}",
                    element_id,
                    element_geometry,
                    element_damage,
                );

                element.draw(&mut frame, element.src(), element_geometry, &element_damage, &log)?;
            }

            Result::<(), R::Error>::Ok(())
        })();

        if let Err(err) = render_res {
            // if the rendering errors on us, we need to be prepared, that this whole buffer was partially updated and thus now unusable.
            // thus clean our old states before returning
            self.last_state = Default::default();
            return Err(DamageTrackedRendererError::Rendering(err));
        }

        Ok((Some(damage), states))
    }

    /// Damage this output and return the damage without actually rendering the difference
    pub fn damage_output<E>(
        &mut self,
        age: usize,
        elements: &[E],
        log: impl Into<Option<slog::Logger>>,
    ) -> Result<(Option<Vec<Rectangle<i32, Physical>>>, RenderElementStates), OutputNoMode>
    where
        E: Element,
    {
        let log = crate::slog_or_fallback(log);

        let (output_size, output_scale, output_transform) = self.mode.clone().try_into()?;
        // We have to apply to output transform to the output size so that the intersection
        // tests in damage_output_internal produces the correct results and do not crop
        // damage with the wrong size
        let output_geo = Rectangle::from_loc_and_size((0, 0), output_transform.transform_size(output_size));

        // This will hold all the damage we need for this rendering step
        let mut damage: Vec<Rectangle<i32, Physical>> = Vec::new();
        let mut render_elements: Vec<&E> = Vec::with_capacity(elements.len());
        let mut opaque_regions: Vec<(usize, Vec<Rectangle<i32, Physical>>)> = Vec::new();
        let states = self.damage_output_internal(
            age,
            elements,
            &log,
            output_scale,
            output_geo,
            &mut damage,
            &mut render_elements,
            &mut opaque_regions,
        );

        if damage.is_empty() {
            Ok((None, states))
        } else {
            Ok((Some(damage), states))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn damage_output_internal<'a, E>(
        &mut self,
        age: usize,
        elements: &'a [E],
        log: &slog::Logger,
        output_scale: Scale<f64>,
        output_geo: Rectangle<i32, Physical>,
        damage: &mut Vec<Rectangle<i32, Physical>>,
        render_elements: &mut Vec<&'a E>,
        opaque_regions: &mut Vec<(usize, Vec<Rectangle<i32, Physical>>)>,
    ) -> RenderElementStates
    where
        E: Element,
    {
        let mut element_render_states = RenderElementStates {
            states: HashMap::with_capacity(elements.len()),
        };

        // We use an explicit z-index because the following loop can skip
        // elements that are completely hidden and we want the z-index to
        // match when enumerating the render elements later
        let mut z_index = 0;
        for element in elements.iter() {
            let element_id = element.id();
            let element_loc = element.geometry(output_scale).loc;

            // First test if the element overlaps with the output
            // if not we can skip it
            let element_output_geometry = match element.geometry(output_scale).intersection(output_geo) {
                Some(geo) => geo,
                None => continue,
            };

            // Then test if the element is completely hidden behind opaque regions
            let element_visible_area = opaque_regions
                .iter()
                .flat_map(|(_, opaque_regions)| opaque_regions)
                .fold([element_output_geometry].to_vec(), |geometry, opaque_region| {
                    geometry
                        .into_iter()
                        .flat_map(|g| g.subtract_rect(*opaque_region))
                        .collect::<Vec<_>>()
                })
                .into_iter()
                .fold(0usize, |acc, item| acc + (item.size.w * item.size.h) as usize);

            // No need to draw a completely hidden element
            if element_visible_area == 0 {
                // We allow multiple instance of a single element, so do not
                // override the state if we already have one
                if !element_render_states.states.contains_key(element_id) {
                    element_render_states
                        .states
                        .insert(element_id.clone(), RenderElementState::skipped());
                }
                continue;
            }

            let element_output_damage = element
                .damage_since(
                    output_scale,
                    self.last_state.elements.get(element.id()).map(|s| s.last_commit),
                )
                .into_iter()
                .map(|mut d| {
                    d.loc += element_loc;
                    d
                })
                .filter_map(|geo| geo.intersection(output_geo))
                .collect::<Vec<_>>();
            damage.extend(element_output_damage);

            let element_opaque_regions = element
                .opaque_regions(output_scale)
                .into_iter()
                .map(|mut region| {
                    region.loc += element_loc;
                    region
                })
                .filter_map(|geo| geo.intersection(output_geo))
                .collect::<Vec<_>>();
            opaque_regions.push((z_index, element_opaque_regions));
            render_elements.push(element);

            if let Some(state) = element_render_states.states.get_mut(element_id) {
                if matches!(state.presentation_state, RenderElementPresentationState::Skipped) {
                    *state = RenderElementState::rendered(element_visible_area);
                } else {
                    state.visible_area += element_visible_area;
                }
            } else {
                element_render_states.states.insert(
                    element_id.clone(),
                    RenderElementState::rendered(element_visible_area),
                );
            }
            z_index += 1;
        }

        // add the damage for elements gone that are not covered an opaque region
        let elements_gone = self
            .last_state
            .elements
            .iter()
            .filter(|(id, _)| !render_elements.iter().any(|e| e.id() == *id))
            .flat_map(|(_, state)| {
                opaque_regions
                    .iter()
                    .filter(|(z_index, _)| state.last_instances.iter().any(|i| *z_index < i.last_z_index))
                    .flat_map(|(_, opaque_regions)| opaque_regions)
                    .fold(
                        state
                            .last_instances
                            .iter()
                            .map(|i| i.last_geometry)
                            .collect::<Vec<_>>(),
                        |damage, opaque_region| {
                            damage
                                .into_iter()
                                .flat_map(|damage| damage.subtract_rect(*opaque_region))
                                .collect::<Vec<_>>()
                        },
                    )
            })
            .collect::<Vec<_>>();
        damage.extend(elements_gone);

        // if the element has been moved or it's z index changed damage it
        for (z_index, element) in render_elements.iter().enumerate() {
            let element_geometry = element.geometry(output_scale);
            let element_last_state = self.last_state.elements.get(element.id());

            if element_last_state
                .map(|s| !s.instance_matches(element_geometry, z_index))
                .unwrap_or(true)
            {
                let mut element_damage = vec![element_geometry];
                if let Some(state) = element_last_state {
                    element_damage.extend(state.last_instances.iter().map(|i| i.last_geometry));
                }
                damage.extend(
                    opaque_regions
                        .iter()
                        .filter(|(index, _)| *index < z_index)
                        .flat_map(|(_, opaque_regions)| opaque_regions)
                        .fold(element_damage, |damage, opaque_region| {
                            damage
                                .into_iter()
                                .flat_map(|damage| damage.subtract_rect(*opaque_region))
                                .collect::<Vec<_>>()
                        }),
                );
            }
        }

        if self
            .last_state
            .size
            .map(|geo| geo != output_geo.size)
            .unwrap_or(true)
        {
            // The output geometry changed, so just damage everything
            slog::trace!(log, "Output geometry changed, damaging whole output geometry. previous geometry: {:?}, current geometry: {:?}", self.last_state.size, output_geo);
            *damage = vec![output_geo];
        }

        // That is all completely new damage, which we need to store for subsequent renders
        let new_damage = damage.clone();

        // We now add old damage states, if we have an age value
        if age > 0 && self.last_state.old_damage.len() >= age {
            slog::trace!(log, "age of {} recent enough, using old damage", age);
            // We do not need even older states anymore
            self.last_state.old_damage.truncate(age);
            damage.extend(self.last_state.old_damage.iter().flatten().copied());
        } else {
            slog::trace!(
                log,
                "no old damage available, re-render everything. age: {} old_damage len: {}",
                age,
                self.last_state.old_damage.len(),
            );
            // just damage everything, if we have no damage
            *damage = vec![output_geo];
        };

        // Optimize the damage for rendering
        damage.dedup();
        damage.retain(|rect| rect.overlaps(output_geo));
        damage.retain(|rect| !rect.is_empty());
        // filter damage outside of the output gep and merge overlapping rectangles
        *damage = damage
            .drain(..)
            .filter_map(|rect| rect.intersection(output_geo))
            .fold(Vec::new(), |new_damage, mut rect| {
                // replace with drain_filter, when that becomes stable to reuse the original Vec's memory
                let (overlapping, mut new_damage): (Vec<_>, Vec<_>) =
                    new_damage.into_iter().partition(|other| other.overlaps(rect));

                for overlap in overlapping {
                    rect = rect.merge(overlap);
                }
                new_damage.push(rect);
                new_damage
            });

        if damage.is_empty() {
            slog::trace!(log, "nothing damaged, exiting early");
            return element_render_states;
        }

        slog::trace!(log, "damage to be rendered: {:#?}", &damage);

        let new_elements_state = render_elements.iter().enumerate().fold(
            IndexMap::<Id, ElementState>::with_capacity(render_elements.len()),
            |mut map, (z_index, elem)| {
                let id = elem.id();
                let elem_geometry = elem.geometry(output_scale);

                if let Some(state) = map.get_mut(id) {
                    state.last_instances.push(ElementInstanceState {
                        last_geometry: elem_geometry,
                        last_z_index: z_index,
                    });
                } else {
                    let current_commit = elem.current_commit();
                    map.insert(
                        id.clone(),
                        ElementState {
                            last_commit: current_commit,
                            last_instances: vec![ElementInstanceState {
                                last_geometry: elem_geometry,
                                last_z_index: z_index,
                            }],
                        },
                    );
                }

                map
            },
        );

        self.last_state.size = Some(output_geo.size);
        self.last_state.elements = new_elements_state;
        self.last_state.old_damage.push_front(new_damage);

        element_render_states
    }
}
