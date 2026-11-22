//! Plugins « hors OBS » des touches : raccourci clavier / macro, lancer une appli ou un site,
//! soundboard et compte à rebours. Chacun n'utilise que des fonctions intégrées à Windows et
//! ne consomme rien tant que la touche n'est pas appuyée.

pub mod countdown;
pub mod hotkey;
pub mod launch;
pub mod sound;
