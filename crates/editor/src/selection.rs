#[derive(Clone, Debug, Default)]
pub(crate) struct Selection {
    entities: Vec<scene::Entity>,
    anchor: Option<scene::Entity>,
}

impl Selection {
    pub(crate) fn clear(&mut self) {
        self.entities.clear();
        self.anchor = None;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.entities.len()
    }

    pub(crate) fn entities(&self) -> &[scene::Entity] {
        &self.entities
    }

    pub(crate) fn primary(&self) -> Option<scene::Entity> {
        self.entities.last().copied()
    }

    pub(crate) fn contains(&self, entity: scene::Entity) -> bool {
        self.entities.contains(&entity)
    }

    pub(crate) fn select_only(&mut self, entity: scene::Entity) {
        self.entities.clear();
        self.entities.push(entity);
        self.anchor = Some(entity);
    }

    pub(crate) fn toggle(&mut self, entity: scene::Entity) {
        if let Some(index) = self.entities.iter().position(|item| *item == entity) {
            self.entities.remove(index);
        } else {
            self.entities.push(entity);
        }
        self.anchor = Some(entity);
    }

    pub(crate) fn select_range(
        &mut self,
        order: &[scene::Entity],
        entity: scene::Entity,
        extend: bool,
    ) {
        let Some(anchor) = self.anchor.filter(|anchor| order.contains(anchor)) else {
            self.select_only(entity);
            return;
        };
        let Some(from) = order.iter().position(|item| *item == anchor) else {
            self.select_only(entity);
            return;
        };
        let Some(to) = order.iter().position(|item| *item == entity) else {
            return;
        };
        let (start, end) = if from <= to { (from, to) } else { (to, from) };
        let mut range = order[start..=end].to_vec();
        if let Some(index) = range.iter().position(|item| *item == entity) {
            range.remove(index);
        }
        range.push(entity);
        if extend {
            for item in range {
                if let Some(index) = self.entities.iter().position(|selected| *selected == item) {
                    self.entities.remove(index);
                }
                self.entities.push(item);
            }
        } else {
            self.entities = range;
        }
    }

    pub(crate) fn set(&mut self, entities: Vec<scene::Entity>) {
        self.anchor = entities.last().copied();
        self.entities = entities;
    }

    pub(crate) fn retain_valid(&mut self, scene: &scene::Scene) {
        self.entities.retain(|entity| scene.local(*entity).is_ok());
        if self
            .anchor
            .is_some_and(|entity| scene.local(entity).is_err())
        {
            self.anchor = self.entities.last().copied();
        }
    }
}
