use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    marker::PhantomData,
};

use amethyst_core::{
    ecs::{
        storage::GenericReadStorage, Component, DenseVecStorage, Entities, Entity, Join, Read,
        ReadExpect, ReadStorage, ReaderId, System, SystemData, Write, WriteStorage,
    },
    math::Vector2,
    shrev::EventChannel,
    Hidden, HiddenPropagate, ParentHierarchy,
};
use amethyst_derive::SystemDesc;
use amethyst_input::{BindingTypes, InputHandler};
use amethyst_window::ScreenDimensions;

use crate::{targeted_below, Interactable, ScaleMode, UiEvent, UiEventType, UiTransform};

/// Component that denotes whether a given ui widget is draggable.
/// Requires UiTransform to work, and its expected way of usage is
/// through UiTransformData prefab.
#[derive(Debug, Serialize, Deserialize)]
pub struct Draggable;

impl Component for Draggable {
    type Storage = DenseVecStorage<Self>;
}

#[derive(Debug, SystemDesc)]
#[system_desc(name(DragWidgetSystemDesc))]
pub struct DragWidgetSystem<T: BindingTypes> {
    #[system_desc(event_channel_reader)]
    ui_reader_id: ReaderId<UiEvent>,

    /// hashmap whose keys are every entities being dragged,
    /// and whose element is a tuple whose first element is
    /// the original mouse position when drag first started,
    /// and second element the mouse position one frame ago
    #[system_desc(skip)]
    record: HashMap<Entity, (Vector2<f32>, Vector2<f32>)>,

    phantom: PhantomData<T>,
}

impl<T> DragWidgetSystem<T>
where
    T: BindingTypes,
{
    pub fn new(ui_reader_id: ReaderId<UiEvent>) -> Self {
        Self {
            ui_reader_id,
            record: HashMap::new(),
            phantom: PhantomData,
        }
    }
}

impl<'s, T> System<'s> for DragWidgetSystem<T>
where
    T: BindingTypes,
{
    type SystemData = (
        Entities<'s>,
        Read<'s, InputHandler<T>>,
        ReadExpect<'s, ScreenDimensions>,
        ReadExpect<'s, ParentHierarchy>,
        ReadStorage<'s, Hidden>,
        ReadStorage<'s, HiddenPropagate>,
        ReadStorage<'s, Draggable>,
        ReadStorage<'s, Interactable>,
        Write<'s, EventChannel<UiEvent>>,
        WriteStorage<'s, UiTransform>,
    );

    fn run(
        &mut self,
        (
            entities,
            input_handler,
            screen_dimensions,
            hierarchy,
            hiddens,
            hidden_props,
            draggables,
            interactables,
            mut ui_events,
            mut ui_transforms,
        ): Self::SystemData,
    ) {
        let mouse_pos = input_handler.mouse_position().unwrap_or((0., 0.));
        let mouse_pos = Vector2::new(mouse_pos.0, screen_dimensions.height() - mouse_pos.1);

        let mut click_stopped: HashSet<Entity> = HashSet::new();

        for event in ui_events.read(&mut self.ui_reader_id) {
            match event.event_type {
                UiEventType::ClickStart => {
                    if draggables.get(event.target).is_some() {
                        self.record.insert(event.target, (mouse_pos, mouse_pos));
                    }
                }
                UiEventType::ClickStop => {
                    if self.record.contains_key(&event.target) {
                        click_stopped.insert(event.target);
                    }
                }
                _ => (),
            }
        }

        for (entity, _) in self.record.iter() {
            if hiddens.get(*entity).is_some() || hidden_props.get(*entity).is_some() {
                click_stopped.insert(*entity);
            }
        }

        for (entity, (first, prev)) in self.record.iter_mut() {
            ui_events.single_write(UiEvent::new(
                UiEventType::Dragging {
                    offset_from_mouse: mouse_pos - *first,
                    new_position: mouse_pos,
                },
                *entity,
            ));

            let change = mouse_pos - *prev;

            let (scale_x, scale_y) =
                get_scale_for_entity(*entity, &hierarchy, &ui_transforms, &screen_dimensions);

            let ui_transform = ui_transforms.get_mut(*entity).unwrap();
            ui_transform.local_x += scale_x * change[0];
            ui_transform.local_y += scale_y * change[1];

            *prev = mouse_pos;
        }

        for entity in click_stopped.iter() {
            ui_events.single_write(UiEvent::new(
                UiEventType::Dropped {
                    dropped_on: targeted_below(
                        (mouse_pos[0], mouse_pos[1]),
                        ui_transforms.get(*entity).unwrap().global_z,
                        (
                            &*entities,
                            &ui_transforms,
                            interactables.maybe(),
                            !&hiddens,
                            !&hidden_props,
                        )
                            .join(),
                    ),
                },
                *entity,
            ));

            self.record.remove(entity);
        }
    }
}

/// Compose the scaling factors of all ancestors until reaching the root (screen) or an entity
/// that doesn't need rescaling (is ScaleMode::Pixel).
fn get_scale_for_entity<S: GenericReadStorage<Component = UiTransform>>(
    entity: Entity,
    hierarchy: &ParentHierarchy,
    ui_transforms: &S,
    screen_dimensions: &ScreenDimensions,
) -> (f32, f32) {
    match ui_transforms.get(entity).unwrap().scale_mode {
        ScaleMode::Pixel => (1.0, 1.0),
        ScaleMode::Percent => {
            get_scale_for_percent_mode_entity(entity, hierarchy, ui_transforms, screen_dimensions)
        }
    }
}

fn get_scale_for_percent_mode_entity<S: GenericReadStorage<Component = UiTransform>>(
    entity: Entity,
    hierarchy: &ParentHierarchy,
    ui_transforms: &S,
    screen_dimensions: &ScreenDimensions,
) -> (f32, f32) {
    let mut parent_width = screen_dimensions.width();
    let mut parent_height = screen_dimensions.height();

    let (parent_scale_x, parent_scale_y) = if let Some(parent) = hierarchy.parent(entity) {
        if let Some(ui_transform) = ui_transforms.get(parent) {
            parent_width = ui_transform.width;
            parent_height = ui_transform.height;
        }

        get_scale_for_entity(parent, hierarchy, ui_transforms, screen_dimensions)
    } else {
        (1.0, 1.0)
    };

    (
        parent_scale_x / parent_width,
        parent_scale_y / parent_height,
    )
}
