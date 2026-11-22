//! Plugins « hors OBS » des touches et de l'écran latéral : raccourci clavier / macro, texte,
//! lancer une appli ou un site, soundboard, compte à rebours, compteur, micro Windows, infos PC.
//! Chacun n'utilise que des fonctions intégrées à Windows et ne consomme rien tant qu'il
//! n'est ni appuyé ni affiché.

pub mod counter;
pub mod countdown;
pub mod hotkey;
pub mod launch;
pub mod mic;
pub mod sound;
pub mod system;
pub mod text;
